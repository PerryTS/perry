//! Constraint generation, part 1b: the export side (cross-module links,
//! contracts and store effects).

use super::*;

/// What a published value no contract describes escapes with (DECISIONS
/// 203): unlike a publication (`Model::pub_reason`), nothing it reaches is
/// read through a contract.
pub(crate) const UNDESCRIBED: &str = "published to another unit, described by no contract";

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
        // The names each module exports itself: they shadow every `export *`
        // (ECMAScript ResolveExport, §16.2.1.6.3; DECISIONS 215).
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
            let ns = self.mods[mi].ns_tok;
            for n in names {
                let a = self.m.atom(n);
                own.insert((mi as u32, a));
                self.m.ns_own.insert((ns, a));
            }
        }
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
                    // Every export of `t` is visible through `mi` under its
                    // own name (DECISIONS 215).
                    Export::ExportAll { source } => match self.resolve_source(mi, module, source) {
                        Some(t) => {
                            let tns = self.mods[t as usize].ns_tok;
                            self.m.export_star(ns, tns);
                        }
                        None => self.m.export_star(ns, TOP),
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
        // through `export *` chains: the namespace's key of that name (each
        // `export *` source's key of the name, DECISIONS 215). A name the
        // module declares itself is not read through them (`own`).
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
        self.m.pub_reason = Some(r);
        if let Some(p) = published {
            self.m.broken_fns = p
                .broken
                .iter()
                .filter(|d| d.starts_with("fn:"))
                .cloned()
                .collect();
        }
        let path_of = |g: &Self, mi: u32| g.modules[mi as usize].0;
        let mut exports: Vec<((u32, Atom), NodeId)> =
            self.exports.iter().map(|(&k, &n)| (k, n)).collect();
        exports.sort_unstable();
        // Census (DECISIONS 59): the method keys a subclass of another unit
        // overrides, per published class (`Gen::override_reports`).
        if let Some(p) = published {
            let classes: Vec<(String, TmplId)> = self
                .export_class
                .values()
                .map(|&tm| (self.m.tmpls[tm as usize].display.clone(), tm))
                .collect();
            for d in &p.broken {
                let Some((k, disp)) = d
                    .strip_prefix("override ")
                    .and_then(|r| r.split_once(" of "))
                else {
                    continue;
                };
                let k = if k == "*" { crate::STAR_KEY } else { k };
                for (_, tm) in classes.iter().filter(|(cd, _)| cd == disp) {
                    let a = self.m.atom(k);
                    let ks = self.m.override_keys.entry(*tm).or_default();
                    if !ks.contains(&a) {
                        ks.push(a);
                    }
                }
            }
        }
        if let Some(p) = published {
            let owners: Vec<_> = self
                .m
                .toks
                .iter()
                .enumerate()
                .filter_map(|(t, tok)| {
                    let TokKind::Obj(tm, _) = tok else {
                        return None;
                    };
                    p.broken
                        .contains(&format!("owner:{}", self.m.tmpls[*tm as usize].display))
                        .then_some(t as u32)
                })
                .collect();
            for t in owners {
                let r = self
                    .m
                    .reason("native owner contract revoked by another unit");
                let n = self.m.node(|| "revoked native owner".into());
                self.m.add_tok(n, t);
                self.m.sink_node(n, r);
            }
        }
        // Census (DECISIONS 126): the typed-array fields a reader escapes or
        // marks hard: `nested_fields` sinks them.
        if let Some(p) = published {
            for d in &p.broken {
                if let Some((k, disp)) = d.strip_prefix("ta ").and_then(|r| r.split_once(" of ")) {
                    self.ta_reported.insert((disp.to_string(), k.to_string()));
                }
            }
        }
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
                    let extended = published.is_some_and(|p| p.extended(path_of(self, mi), name));
                    self.publish_contract_class(Some((mi, a)), n, tm, extended, r);
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
                    // A class the census broke escapes as everything did in
                    // P1, beyond any contract (DECISIONS 203).
                    let rb = match self.export_class.contains_key(&(mi, a)) {
                        true => self.m.reason(UNDESCRIBED),
                        false => r,
                    };
                    self.m.sink_node(n, rb)
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
        // Design §17.16 (b): per unit, a registry entry is a cell every unit
        // names the same way. What this unit writes leaves through the
        // cell's contract (checked as a published value, `contract_check`);
        // what the other units write is seeded. A path no unit writes holds
        // nothing, as in the whole-program solve.
        let cells = published.is_some() && self.unit_in.is_some();
        for (k, n) in paths {
            if cells {
                let key = format!("cjs:{k}");
                self.seed_cell(&key, n);
                self.cells.push((key, n));
                continue;
            }
            if foreign(self, &k) {
                self.m
                    .add_top(n, |_| "CommonJS module of another unit".into());
            }
            self.m.sink_node(n, r);
        }
        // The registry union and the unknown-path registry are cells
        // (DECISIONS 171). DECISIONS 188: `require.cache` and the main record
        // are the support module's `cache` and `main` (`perry:cjs`), born
        // there once for the program: per unit each holds that binding's
        // value, the unit's own where it binds it, else its value contract's
        // stand-in; what a unit stores into them is a store effect its owner
        // performs (decision 17), so they leave through no ⊤. Without a
        // support module, or with no contract of it, they are the program's,
        // shared with every unit: ⊤.
        let mut shared: Vec<NodeId> = Vec::new();
        for (n, name) in [(self.m.cjs_cache, "cache"), (self.m.cjs_main, "main")] {
            let support = self.unit_in.and_then(|u| u.cjs_support.as_deref());
            let own = support.and_then(|sp| {
                self.exports
                    .iter()
                    .find(|(&(mi, a), _)| {
                        path_of(self, mi) == sp && self.m.atoms[a as usize] == name
                    })
                    .map(|(_, &b)| b)
            });
            let value = match (cells, own, support) {
                (true, Some(b), _) => Some(b),
                (true, None, Some(sp)) => self
                    .value_contract_for(sp, name)
                    .map(|vc| self.stand_in_value(0, name, vc)),
                _ => None,
            };
            match value {
                Some(v) => self.m.edge(v, n),
                None => shared.push(n),
            }
        }
        for (key, n) in [
            ("cjs:*", self.cjs_registry),
            ("cjs:?", self.cjs_unknown_path),
        ] {
            if cells {
                self.seed_cell(key, n);
                self.cells.push((key.to_string(), n));
            } else {
                shared.push(n);
            }
        }
        for n in shared {
            self.m
                .add_top(n, |_| "CommonJS registry shared with other units".into());
            self.m.sink_node(n, r);
        }
        // A thrown value reaches the units whose `catch` is up the stack
        // (DECISIONS 174): per unit it is one cell, `thrown`, every unit
        // names alike, as a registry entry is: what this unit throws leaves
        // through the cell's contract, what the others throw is seeded into
        // what its `catch` reads (beside the runtime's errors, ⊤).
        let thrown = self.thrown;
        if cells {
            self.seed_cell("thrown", thrown);
            self.cells.push(("thrown".to_string(), thrown));
        } else {
            self.m.sink_node(thrown, r);
        }
        // Other units' code reaches the global object and stores into what
        // this unit gives them: their writes, which they publish
        // (`Model::shared_replay`).
        let gt = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        if cells {
            // Design §17.16 (b): a program-owned global is a cell as well.
            // Every key this unit can name is an atom by now (a later one:
            // `builtins::global_field`). The global object itself is no value
            // of another unit: every unit reads it, none publishes it.
            let mut keys: Vec<(String, Atom)> = (0..self.m.atoms.len() as Atom)
                .filter(|&a| crate::builtins::global_user_key(&self.m.atoms[a as usize]))
                .map(|a| (self.m.atoms[a as usize].clone(), a))
                .collect();
            keys.sort_unstable();
            for (k, a) in keys {
                let n = crate::builtins::global_field(&mut self.m, gt, a);
                let key = format!("global:{k}");
                self.seed_cell(&key, n);
                self.cells.push((key, n));
            }
            self.m.global_cells = true;
        } else {
            self.m.shared_replay = true;
            self.m.escape(gt, r);
            self.m.shared_replay = false;
        }
        let holder = self.m.node_l("other-unit-receiver");
        self.m.other_units_store = Some(holder);
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
            let name = match nm.contains('#')
                && !self.m.funcs[fx as usize].name.ends_with(&format!("#{fid}"))
            {
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
            if ac.is_some() {
                self.m.closed_called_out.insert(fx);
            }
            let mut nodes = Vec::with_capacity(n);
            for i in 0..n {
                // No contract yet is no caller input (decision 13), not a
                // call with omitted arguments. A present caller contract with
                // a missing position does contribute undefined (77).
                let node = match ac.and_then(|a| a.params.get(i)) {
                    Some(vc) => self.stand_in_value(0, &format!("{nm}:arg{i}"), vc),
                    None => {
                        let node = self.m.node_named("arg", || format!("arg{i}({nm})"));
                        // Constructor initialization keeps its existing
                        // conservative envelope; DEC223 forwards ordinary calls.
                        if ac.is_some() || self.m.funcs[fx as usize].ctor_of.is_some() {
                            self.m.add_prim(node, prim::UNDEF);
                        }
                        node
                    }
                };
                nodes.push(node);
            }
            self.m.closed_params.insert(fx, nodes);
            // A closed function passed as an argument (decisions 19/23),
            // including an own function inside its caller SCC: published
            // where it is created, as a contract function through its closed
            // contract (markers on its parameters, not an escape), so a
            // passing call's argument contract can describe it. DECISIONS
            // 195: only one passed by name as an argument can reach another
            // unit; any other is called here alone, its parameters hold its
            // callers' values (as in the whole program) and carry no markers.
            if view.closed_passed.contains(nm) {
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

    /// A class of this unit becomes a contract (§11.2): published, its
    /// instance's fields leave. `key` is its export; none for a class whose
    /// instance a contract result or value gives out (DECISIONS 116).
    fn publish_contract_class(
        &mut self,
        key: Option<(u32, Atom)>,
        n: NodeId,
        tm: TmplId,
        extended: bool,
        r: u32,
    ) -> bool {
        // No constructor code runs on the receiver but the field
        // initializers, which store the class's own keys and do
        // not leak `this` (DECISIONS 58).
        let t = &self.m.tmpls[tm as usize];
        let no_code = t.ctor.is_none()
            && t.parent.is_none()
            && t.parent_value.is_none()
            && t.native_parent.is_none()
            && self.ctor_phase.get(&tm).is_some_and(|ph| !ph.leaks);
        let statics = self
            .m
            .publish_class(tm, r, extended, no_code, key.is_none());
        for (a, sn) in statics {
            if !self.static_values.contains(&(tm, a, sn)) {
                self.static_values.push((tm, a, sn));
            }
        }
        self.class_contracts.push((key, n, tm));
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        self.fields_leave(inst, r)
    }

    /// DECISIONS 116: a class of this unit that no other unit names, whose
    /// instance a contract result or value gives out (AWFY's
    /// `exports.newInstance = () => new Richards()`): the reader calls its
    /// methods on what it gets, as on an exported class's instance, so it
    /// is the same contract, published under its display. No other unit
    /// can name it, so none extends it. A census report breaks it as any.
    fn class_to_give(&self, tm: TmplId) -> bool {
        let t = &self.m.tmpls[tm as usize];
        matches!(
            t.kind,
            TmplKind::Class {
                literal_shape: false,
                ..
            }
        ) && !self.stand_in_cc.contains_key(&tm)
            && self.unit_in.is_some_and(|u| !u.broken(&t.display))
    }

    /// Gives `t` (an instance of contract class `tm`, or of a class to
    /// give, published here first) out to other units. True when this
    /// added constraints.
    fn give_class(&mut self, tm: TmplId, t: Tok, r: u32) -> bool {
        let mut changed = false;
        if !self.contract_class(tm) {
            self.publish_contract_class(None, u32::MAX, tm, false, r);
            changed = true;
        }
        changed | self.m.give_out(tm, t)
    }

    /// After a solve: a published class export holds its class value only
    /// (a module may rebind the name); otherwise it escapes after all. The
    /// primitives beside it are the contract's `binding_prim` (DECISIONS
    /// 115): no object, so the class is what every reader may find. True
    /// when something changed (solve again).
    /// A value contract function `f` hands the caller's unit (its result,
    /// or an argument of a call through its parameter, DECISIONS 140), node
    /// `n`: `None` if no contract describes what it holds, else whether
    /// publishing it changed anything (its fields leave, classes go out).
    fn publish_result(&mut self, f: FnIdx, n: NodeId, r: u32) -> Option<bool> {
        let mut changed = false;
        // A marker of no family of this run (the result's own marker,
        // standing for what crossed into it, MRK) is another call's value: a
        // parameter of `f` it stands for was this run's argument in some
        // other call, not this one's. It says no more than ⊤ only when
        // everything it stands for reads as ⊤.
        let frame = self.m.nodes[n as usize].frame;
        let held = self.m.nodes[n as usize].pts.to_sorted();
        let foreign = held.iter().any(|&t| {
            self.m.is_arg_marker(t)
                && (self.m.fam_home(t) == 0 || self.m.fam_home(t) != frame)
                && !self
                    .m
                    .marker_members(t)
                    .iter()
                    .all(|&x| self.m.marker_is_top(x))
        });
        if foreign {
            return None;
        }
        // An argument marker reads as the markers it stands for (DECISIONS 44).
        let toks: Vec<Tok> = held
            .into_iter()
            .flat_map(|t| self.m.marker_members(t))
            .collect();
        // Another function's marker with nothing beside it (DECISIONS
        // 119) is another unit's value the caller cannot follow: it
        // escapes, then reads as ⊤ (DECISIONS 42).
        for &t in &toks {
            if self.m.param_marker.get(&t).is_some_and(|&(g, _)| g != f)
                && !self.m.beside_values(t)
                && !self.m.marker_is_top(t)
            {
                self.m.escape(t, r);
                changed = true;
            }
        }
        // A value no contract describes escapes alone (DECISIONS 174, 175):
        // the result still describes the others, and its contract says
        // unknown beside them (`describe_nodes`). Nothing described at all
        // and the result is unknown (`ret_top`).
        let (toks, bad): (Vec<Tok>, Vec<Tok>) =
            toks.into_iter().partition(|&t| self.result_tok_ok(f, t));
        if !bad.is_empty() && toks.is_empty() {
            return None;
        }
        for t in bad {
            changed |= self.m.escape_changed(t, r);
        }
        for t in toks {
            if matches!(
                self.m.toks[t as usize],
                TokKind::Top | TokKind::BuiltinFn | TokKind::Builtin(_)
            ) || self.m.param_marker.contains_key(&t)
            {
                continue;
            }
            if self.arr_ok(t) {
                changed |= self.arr_leaves(t);
                continue;
            }
            // DECISIONS 147: the caller's unit calls a function it is
            // handed through its contract (§11.2, as a published binding's,
            // `check_value`): its parameters carry markers, its result is
            // checked; a closed one leaves through its argument contract.
            // Another unit's function leaves with its owner's contract.
            if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                if self.m.funcs[f as usize].func_id != u32::MAX {
                    let new = self.m.contract_fns.insert(f);
                    changed |= new;
                    let rf = match self.m.closed_params.contains_key(&f) {
                        true => self.m.closed_pub_reason.unwrap_or(r),
                        false => r,
                    };
                    // One that escaped already, as no contract function,
                    // gets its markers now (as a published field function,
                    // `note_field_fns`).
                    match self.m.escaped.get(&t).copied() {
                        Some(r0) if new => self.m.escape_fn_params(t, r0),
                        _ => self.m.escape(t, rf),
                    }
                }
                continue;
            }
            if self.stand_in_fc.contains_key(&t) {
                continue;
            }
            if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                if self.contract_class(tm) || self.class_to_give(tm) {
                    changed |= self.give_class(tm, t, r);
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
        Some(changed)
    }

    /// One value a contract function `f` hands the caller's unit
    /// (`publish_result`): whether a contract describes it.
    pub(crate) fn result_tok_ok(&self, f: FnIdx, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            _ if self.m.param_marker.get(&t).is_some_and(|&(g, _)| g == f) => true,
            // An escaped marker reads as ⊤ (DECISIONS 42), of any
            // contract function: another unit's value.
            _ if self.m.marker_is_top(t) => true,
            // An unknown value or a builtin function in the result is
            // the caller's ⊤, nothing of this unit leaves through it.
            TokKind::Top | TokKind::BuiltinFn => true,
            // A builtin, a program object, an array (DECISIONS 94): one
            // runtime object every unit names alike, or an array whose
            // elements are a cell (`arr_leaves` below).
            TokKind::Builtin(_) => true,
            // DECISIONS 147: a function of this unit is a contract function
            // (below); another unit's leaves with its owner's contract, as
            // a stand-in instance does, unless it is closed: a closed
            // contract's users are known to its owner (`Model::escape`).
            TokKind::Func(f, _) if self.m.funcs[f as usize].func_id != u32::MAX => true,
            TokKind::Func(f, _) if self.stand_in_fc.contains_key(&t) => {
                self.m.funcs[f as usize].forwarded
            }
            TokKind::Obj(..) if self.arr_ok(t) => true,
            // An instance of a contract class: given out (`give_out`),
            // or of a class to give (DECISIONS 116).
            TokKind::Obj(tm, _) if self.contract_class(tm) || self.class_to_give(tm) => true,
            // Another unit's instance (a stand-in): forwarded with that
            // unit's contract (`fn_contract_of`), its proof (§17.10 4).
            TokKind::Obj(tm, _) if self.stand_in_cc.contains_key(&tm) => !self
                .unit_in
                .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display)),
            TokKind::Obj(tm, _) => self.literal_ok(tm),
            _ => false,
        }
    }

    pub(crate) fn contract_check(&mut self) -> bool {
        let r = self.m.reason("published to another unit");
        let mut changed = self.apply_store_effects();
        for (e, n, tm) in std::mem::take(&mut self.class_contracts) {
            let c = self.m.tok(TokKind::ClassObj(tm));
            // A class given out has no binding (DECISIONS 116).
            if e.is_none() {
                self.class_contracts.push((e, n, tm));
                continue;
            }
            let node = &self.m.nodes[n as usize];
            if node.pts.to_sorted().iter().any(|&t| t != c) {
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
            match self.publish_result(f, ret, r) {
                Some(ch) => changed |= ch,
                None => {
                    self.ret_top.insert(f);
                    self.m.sink_node(ret, r);
                    changed = true;
                }
            }
        }
        // DECISIONS 176: what a contract function stores of this unit into
        // the caller's values goes there as its result does.
        let mut flows: Vec<(crate::model::StoreFlowKey, NodeId)> = self
            .m
            .store_flows
            .iter()
            .filter(|(_, fl)| !fl.vals_top)
            .filter_map(|(&k, fl)| Some((k, fl.vals?)))
            .collect();
        flows.sort_unstable_by_key(|&(_, v)| v);
        for (k, v) in flows {
            match self.publish_result(k.0, v, r) {
                Some(ch) => changed |= ch,
                None => {
                    if let Some(fl) = self.m.store_flows.get_mut(&k) {
                        fl.vals_top = true;
                    }
                    self.m.sink_node(v, r);
                    changed = true;
                }
            }
        }
        // DECISIONS 140: the arguments of a call through a parameter marker
        // go to the caller's unit as the result does; undescribed, or once
        // the marker escapes (the caller's function is then unknown code),
        // they escape as before.
        let calls = self.m.marker_calls.clone();
        for (f, i, _, call) in calls {
            let top = self
                .m
                .marker_of
                .get(&(f, i))
                .is_some_and(|&t| self.m.marker_is_top(t));
            for a in self.m.calls[call as usize].args.clone() {
                if self.call_arg_top.contains(&a) {
                    continue;
                }
                match (!top).then(|| self.publish_result(f, a, r)).flatten() {
                    Some(ch) => changed |= ch,
                    None => {
                        self.call_arg_top.insert(a);
                        let ru = self
                            .m
                            .reason("passed to a call of an unknown function value");
                        self.m.sink_node(a, ru);
                        changed = true;
                    }
                }
            }
        }
        // A published value binding: what it may hold must be described by
        // a contract (a literal-shape object of this unit whose fields then
        // leave, an instance of a contract class, another unit's stand-in,
        // an unknown value); anything else and it escapes as before.
        // The cells this unit writes (design §17.16 (b)): checked as a
        // published value binding; undescribed, sunk and published unknown.
        for i in 0..self.cells.len() {
            let n = self.cells[i].1;
            if self.cell_bad.contains(&n) {
                continue;
            }
            changed |= self.cell_flows_of(i);
            let (ok, ch) = self.check_value(n, r);
            changed |= ch;
            if !ok {
                self.cell_bad.insert(n);
            }
        }
        for (e, n) in std::mem::take(&mut self.value_contracts) {
            let (ok, ch) = self.check_value(n, r);
            changed |= ch;
            if ok {
                self.value_contracts.push((e, n));
            }
        }
        // A published class's static member is a value binding of the class
        // (DECISIONS 173): checked the same way; undescribed, it is sunk and
        // its readers find an unknown value.
        for (tm, a, n) in std::mem::take(&mut self.static_values) {
            let (ok, ch) = self.check_value(n, r);
            changed |= ch;
            if ok {
                self.static_values.push((tm, a, n));
            }
        }
        // Decision 17 with its value (design §17.16 (c)): what this unit
        // stores into another unit's object is a published value, checked
        // as a value binding is; undescribed, it escapes as before.
        let mut stored: Vec<NodeId> = self.m.foreign_stores.iter().map(|&(_, _, n)| n).collect();
        stored.sort_unstable();
        stored.dedup();
        for n in stored {
            if self.effect_bad.contains(&n) {
                continue;
            }
            let (ok, ch) = self.check_value(n, r);
            changed |= ch;
            if !ok {
                self.effect_bad.insert(n);
            }
        }
        // What this unit writes into other units' bindings (`local_write`):
        // a value contract as a published binding's, its values leave the
        // same way.
        let mut writes: Vec<(u32, NodeId)> = self
            .binding_writes
            .iter()
            .map(|(&id, &n)| (id, n))
            .filter(|(id, _)| !self.write_top.contains(id))
            .collect();
        writes.sort_unstable();
        for (id, n) in writes {
            let (ok, ch) = self.check_value(n, r);
            changed |= ch;
            if !ok {
                self.write_top.insert(id);
                changed = true;
            }
        }
        // DECISIONS 210: the instances constructed here for the units that
        // construct this unit's functions (`published_ctors`): their layout
        // is published to them as a class given out is (`give_class`): the
        // prototype's functions are contracts run on them, the fields leave
        // (`fn_inst_contract` describes them). One the census broke (a
        // reader used it beyond the layout) escapes beyond any contract.
        let runs = self.m.own_ctor_runs.clone();
        for (f, ret) in runs {
            let Some(&tm) = self.m.fn_inst.get(&f) else {
                continue;
            };
            let d = self.m.tmpls[tm as usize].display.clone();
            let broken = self.unit_in.is_some_and(|u| u.broken(&d));
            for t in self.m.nodes[ret as usize].pts.to_sorted() {
                if self.m.tmpl_of(t) != Some(tm) {
                    continue;
                }
                if broken {
                    let rb = self.m.reason(UNDESCRIBED);
                    changed |= self.m.escape_changed(t, rb);
                    continue;
                }
                if self.m.fn_inst_published.insert(tm) {
                    self.m.publish_fn_inst(f, tm, t, r);
                    changed = true;
                }
                if !self.ret_ok.contains(&t) {
                    changed |= self.note_field_fns(t);
                }
                changed |= self.fields_leave(t, r);
            }
        }
        // DEC223: a concrete owned callback handed to a known closed callee
        // is published through the normal value-contract check. This names
        // its callable identity; it does not close its own entry. The check
        // retains unknown captures/effects and refuses an undescribed escape.
        for t in self.closed_arg_functions() {
            changed |= self.check_toks(vec![t], r).1;
            // DECISIONS 224: its runners publish their arguments.
            let f = self.m.tok_fn(t);
            if self.m.contract_fns.contains(&f) {
                self.m.callable_pub.insert(f);
            }
        }
        for t in self.closed_arg_literals() {
            if !self.arr_ok(t) && !self.handed.contains(&t) {
                self.handed.push(t);
            }
        }
        changed |= self.nested_fields(r);
        // A literal of this unit passed to another unit's closed function
        // (decision 13) is described by the argument contract as a value
        // contract describes a published literal: its field functions are
        // contract functions the callee calls through their contract. An
        // array leaves through its element cell, as a published value's and
        // a contract result's do (DECISIONS 94, 207): the argument contract
        // names it, and the callee reads its elements from the cell.
        for t in self.closed_arg_literals() {
            if self.arr_ok(t) {
                changed |= self.arr_leaves(t);
                continue;
            }
            if !self.field_fns.keys().any(|&(ft, _)| ft == t) {
                changed |= self.note_field_fns(t);
            }
        }
        changed
    }

    /// Design §17.16 (b): cell `key` as this unit reads it: node `n` also
    /// holds what the other units write (their contracts' stand-ins), and
    /// an unknown value when a unit solve went over the bound.
    /// DECISIONS 171: a unit reads a cell where its solve consumes the
    /// cell's node (a successor edge or a constraint on it). One nothing
    /// consumes yet is neither seeded nor read: the other units' writes
    /// cannot reach its facts, and what it publishes there is its own
    /// write. The first consumer wakes it (`Model::cell_watch`) and
    /// `wake_cells` seeds it before the unit solves again. Every unit
    /// names the registry union `cjs:*`; on AWFY 18 of 471 consume it.
    pub(crate) fn seed_cell(&mut self, key: &str, n: NodeId) {
        let node = &self.m.nodes[n as usize];
        if self.unit_in.is_some() && node.succ.is_empty() && node.cons.is_empty() {
            self.cells_deferred.push((key.to_string(), n));
            self.m.cell_watch.insert(n);
            return;
        }
        self.seed_cell_now(key, n);
    }

    /// The deferred cells a consumer reached in the last solve, seeded now.
    /// True when there were any: the unit solves again.
    pub(crate) fn wake_cells(&mut self) -> bool {
        let woken = std::mem::take(&mut self.m.cell_woken);
        if woken.is_empty() {
            return false;
        }
        let mut now: Vec<(String, NodeId)> = Vec::new();
        self.cells_deferred.retain(|(k, n)| {
            let w = woken.contains(n);
            if w {
                now.push((k.clone(), *n));
            }
            !w
        });
        for (k, n) in now {
            self.seed_cell_now(&k, n);
        }
        true
    }

    fn seed_cell_now(&mut self, key: &str, n: NodeId) {
        self.cells_read.push(key.to_string());
        // A unit whose globals unmodeled code may write publishes `global:*`
        // unknown: every global cell reads it.
        let wide = key.starts_with("global:").then_some("global:*");
        if let Some(w) = wide {
            self.cells_read.push(w.to_string());
        }
        let Some(u) = self.unit_in else { return };
        let me = self.view.map(|v| v.key.clone()).unwrap_or_default();
        let seed = self.m.node_named("cell", || format!("cell({key})"));
        self.cell_seeds.insert(n, seed);
        if u.cells_top {
            self.m
                .add_top(seed, |_| "cell written by a unit over the bound".into());
            self.m.add_prim(seed, prim::ALL);
        }
        let writes = u.cells.get(key).into_iter().flatten();
        let wide = wide.and_then(|w| u.cells.get(w)).into_iter().flatten();
        // One stand-in per distinct write (DECISIONS 113); this unit's own
        // write is in its own solve (DECISIONS 89).
        for w in writes.chain(wide) {
            if w.writers.len() == 1 && w.writers[0] == me {
                continue;
            }
            let v = self.stand_in_value(0, key, &w.c);
            if n == self.thrown {
                let held = self.m.nodes[v as usize].pts.to_sorted();
                self.m.thrown_in.extend(held);
            }
            self.m.edge(v, seed);
        }
        self.m.edge(seed, n);
    }

    /// DECISIONS 162: array cell `cells[i]` holding a marker that stands
    /// for own parameters of this unit's contract functions, none escaped
    /// (`Model::own_param_members`): the body stored the callers' argument
    /// into a shared array, which their units' solves hold, not this one.
    /// The marker is described by a flow (`FnContract::param_cells`): each
    /// caller's unit stores its argument into its view of the cell, as the
    /// whole program runs that store. A marker that later turns ⊤ is no
    /// longer described: the cell check sinks it then. Only the elements of
    /// an array: a named field of a shared array, a global or a program
    /// object cell keep the check. True when it recorded a new flow.
    fn cell_flows_of(&mut self, i: usize) -> bool {
        let (key, n) = (&self.cells[i].0, self.cells[i].1);
        if !(key.starts_with("arr:") || key.starts_with("box:")) || key.ends_with("#named") {
            return false;
        }
        let key = key.clone();
        let mut changed = false;
        for t in self.m.nodes[n as usize].pts.to_sorted() {
            if !self.m.is_marker(t) || self.m.beside_values(t) {
                continue;
            }
            let Some(own) = self.m.own_param_members(t) else {
                self.cell_flow_toks.remove(&t);
                continue;
            };
            self.cell_flow_toks.insert(t);
            for (f, p) in own {
                changed |= self.cell_flows.insert((f, p, key.clone()));
            }
        }
        changed
    }

    /// A published value (a binding, a stored value of a store effect):
    /// what it may hold must be described by a contract; anything else and
    /// it escapes (sunk). Returns (described, changed).
    fn check_value(&mut self, n: NodeId, r: u32) -> (bool, bool) {
        let toks = self.m.nodes[n as usize].pts.to_sorted();
        self.check_toks(toks, r)
    }

    /// `check_value` over these values (of one published node).
    fn check_toks(&mut self, toks: Vec<Tok>, r: u32) -> (bool, bool) {
        let mut changed = false;
        {
            // A function of this unit the binding holds, which nothing let
            // escape yet, is a contract function (§11.2, as a published
            // literal's field functions are, `note_field_fns`): other units
            // call it through its contract.
            for &t in &toks {
                if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                    if self.m.funcs[f as usize].func_id != u32::MAX && !self.m.is_escaped(t) {
                        let new = self.m.contract_fns.insert(f);
                        changed |= new;
                        // Another closure value of it escaped already, as no
                        // contract function: its markers now (as
                        // `note_field_fns`).
                        let ft = self.m.funcs[f as usize].token;
                        if let Some(r0) = self.m.escaped.get(&ft).copied().filter(|_| new) {
                            self.m.escape_fn_params(ft, r0);
                        }
                    }
                }
            }
            // A value no contract describes escapes alone (DECISIONS 174):
            // the binding still describes the others, and its contract says
            // unknown beside them (`value_contract_of`). Its readers find an
            // unknown value, so nothing it reaches is read through a
            // contract: it escapes beyond any (DECISIONS 203), and another
            // unit's stand-in it reaches is this unit's report, not a value
            // forwarded with its owner's contract (`Model::escape`).
            let (toks, bad): (Vec<Tok>, Vec<Tok>) =
                toks.into_iter().partition(|&t| self.value_tok_ok(t));
            let ru = self.m.reason(UNDESCRIBED);
            for t in bad {
                changed |= self.m.escape_changed(t, ru);
            }
            for t in toks {
                // Nothing of this unit leaves through a marker (DECISIONS 137).
                if self.m.is_marker(t) {
                    continue;
                }
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
                    if self.contract_class(tm) || self.class_to_give(tm) {
                        changed |= self.give_class(tm, t, r);
                        changed |= self.fields_leave(t, r);
                    } else if self.arr_ok(t) {
                        changed |= self.arr_leaves(t);
                    } else if !self.stand_in_cc.contains_key(&tm) {
                        if !self.ret_ok.contains(&t) {
                            changed |= self.note_field_fns(t);
                        }
                        changed |= self.fields_leave(t, r);
                    }
                }
            }
        }
        (true, changed)
    }

    /// Owned callable values actually reaching parameters of a known closed
    /// stand-in. Token/member IDs stay in this solve; no source-name guess or
    /// new caller envelope is involved (DEC223). DECISIONS 224: a stand-in of
    /// a function published to a known closed callee has known callers too
    /// (each running unit publishes what it passes): what reaches its
    /// parameters is published the same way.
    fn closed_arg_functions(&self) -> Vec<Tok> {
        let mut out: Vec<Tok> = self
            .stand_in_fns
            .iter()
            .filter(|((_, holder), &(_, t))| {
                holder.is_none()
                    && self
                        .stand_in_fc
                        .get(&t)
                        .is_some_and(|fc| fc.closed.is_some() || fc.callable)
            })
            .flat_map(|(_, &(_, t))| self.m.funcs[self.m.tok_fn(t) as usize].params.iter())
            .flat_map(|&p| self.m.nodes[p as usize].pts.to_sorted())
            .flat_map(|t| self.m.marker_members(t))
            .filter(|&t| {
                matches!(self.m.toks[t as usize], TokKind::Func(f, _)
                if self.m.funcs[f as usize].func_id != u32::MAX)
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
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
        self.m.field_fn_reason = Some(r);
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
            if !matches!(t.kind, TmplKind::Class { .. } | TmplKind::Lit { .. })
                || self.stand_in_cc.contains_key(&(tm as TmplId))
            {
                continue;
            }
            for (k, c) in u.stored_keys(&t.display) {
                want.push((tm as TmplId, k, c));
            }
        }
        if want.is_empty() {
            return false;
        }
        let want: Vec<(TmplId, Atom, &'a crate::units::ValueContract)> = want
            .into_iter()
            .map(|(tm, k, c)| (tm, self.m.atom(k), c))
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
            for &(_, a, c) in want.iter().filter(|w| w.0 == tm) {
                if self.effects_done.insert((t, a)) {
                    let src = self.effect_src(tm, a, c);
                    // `*`: a store under a computed key (DECISIONS 188).
                    let key = match self.m.atoms[a as usize] == crate::STAR_KEY {
                        true => crate::model::Key::Any,
                        false => crate::model::Key::Named(a),
                    };
                    self.m.store_into(t, key, src, site);
                    changed = true;
                }
            }
        }
        changed
    }

    /// The value a store effect stores: the stand-in of the joined value
    /// contract the storing units published (design §17.16 (c)), one node
    /// per layout and key.
    fn effect_src(&mut self, tm: TmplId, a: Atom, c: &'a crate::units::ValueContract) -> NodeId {
        if let Some(&n) = self.effect_srcs.get(&(tm, a)) {
            return n;
        }
        let n = self.at_module_scope(|g| g.stand_in_value(0, "stored by another unit", c));
        self.effect_srcs.insert((tm, a), n);
        n
    }

    /// The store effects this unit publishes (decision 17), by template
    /// display and key, with the nodes of what it stores: their value
    /// contracts are published (design §17.16 (c), DECISIONS 73 their kinds).
    pub(crate) fn store_effects(&self) -> Vec<(String, String, Vec<NodeId>)> {
        let mut by: FxHashMap<(TmplId, Atom), Vec<NodeId>> = FxHashMap::default();
        for &(tm, a, n) in &self.m.foreign_stores {
            by.entry((tm, a)).or_default().push(n);
        }
        let mut out: Vec<(String, String, Vec<NodeId>)> = by
            .into_iter()
            .map(|((tm, a), mut ns)| {
                ns.sort_unstable();
                ns.dedup();
                (
                    self.m.tmpls[tm as usize].display.clone(),
                    self.m.atoms[a as usize].clone(),
                    ns,
                )
            })
            .collect();
        out.sort();
        out
    }

    /// A literal-shape object, a plain object literal (DECISIONS 94) or an
    /// enum's object (§18.2) of this unit no census report broke.
    fn literal_ok(&self, tm: TmplId) -> bool {
        let t = &self.m.tmpls[tm as usize];
        matches!(
            t.kind,
            TmplKind::Class {
                literal_shape: true,
                ..
            } | TmplKind::Lit { .. }
        ) && !self.unit_in.is_some_and(|u| u.broken(&t.display))
    }

    /// DECISIONS 94: the name an array is published under: another unit's
    /// array its owner's name, this unit's array its display under this
    /// unit's key (a site ordinal is unique within one unit only).
    pub(crate) fn arr_name(&self, t: Tok) -> String {
        let tm = self.m.tmpl_of(t).expect("array template");
        let d = &self.m.tmpls[tm as usize].display;
        if self.stand_in_arrs.get(d) == Some(&t) {
            return d.clone();
        }
        let me = self.view.map_or("", |v| v.key.as_str());
        format!("{me}:{d}")
    }

    /// DECISIONS 94: an array a value contract describes, this unit's or
    /// another unit's stand-in: its elements leave through cell
    /// `arr:<display>`, so it describes every value it holds.
    fn arr_ok(&self, t: Tok) -> bool {
        self.m.is_cell_arr(t)
    }

    /// DECISIONS 94: array `t` leaves (published, a published field holds
    /// it, or it is another unit's): its elements are cell `arr:<display>`
    /// (what the other units holding it write there is seeded, what this
    /// unit writes is published), its named fields hold ⊤ and sink what is
    /// stored there (`Model::cell_arrs`). True when this added the cell.
    pub(crate) fn arr_leaves(&mut self, t: Tok) -> bool {
        if self.m.cell_arrs.contains_key(&t) {
            return false;
        }
        // DECISIONS 184: a container's cell is `box:<name>`, so a reader
        // knows its kind from the key alone (`FnContract::param_cells`).
        let kind = if self.m.is_box(t) { "box" } else { "arr" };
        let key = format!("{kind}:{}", self.arr_name(t));
        let named = self.m.node_named("cell", || format!("cell({key}#named)"));
        self.seed_cell(&format!("{key}#named"), named);
        self.m.cell_arrs.insert(t, named);
        // Its named fields so far see the seed; what was stored under a
        // named key escapes (`store_into` does it for later stores).
        if let Some(fs) = self.m.field_keys.get(&t).cloned() {
            for a in fs {
                let n = self.m.field(t, a);
                self.m.edge(named, n);
            }
        }
        if let Some(srcs) = self.m.arr_named_srcs.remove(&t) {
            let r = self
                .m
                .reason("stored into a named field of an array shared with other units");
            self.m.arr_named.insert(t);
            for s in srcs {
                self.m.sink_node(s, r);
            }
        }
        let n = self.m.field_any(t);
        // Other units' stores under a computed key are published into the
        // element cell: the named keys read them as well.
        if self.m.is_elem_array(t) {
            let na = self.m.named_any(t);
            self.m.edge(n, na);
        }
        // DECISIONS 184: a container's elements are primitives besides
        // what was stored (`builtins::tok_load`): its cell says so, as a
        // reader's own container of the same kind reads them.
        if self.m.is_box(t) {
            self.m.add_prim(n, crate::model::prim::ALL);
        }
        self.seed_cell(&key, n);
        self.cells.push((key.clone(), n));
        self.arr_cells.push((key, t));
        true
    }

    fn contract_class(&self, tm: TmplId) -> bool {
        self.class_contracts.iter().any(|&(_, _, c)| c == tm)
    }

    /// A token a value contract describes (`contract_check`).
    fn value_tok_ok(&self, t: Tok) -> bool {
        // A runtime-owned object: the contract names it (`ValueContract::shared`).
        if self.m.named_shared(t).is_some() {
            return true;
        }
        if let Some((tm, _)) = crate::bindings::sealed_owner(&self.m, t) {
            let d = &self.m.tmpls[tm as usize].display;
            if !self
                .unit_in
                .is_some_and(|u| u.broken(&format!("owner:{d}")))
            {
                return true;
            }
        }
        match self.m.toks[t as usize] {
            // DECISIONS 137: a marker with values beside it (DECISIONS 74)
            // stands for no value of its own: the callers' values arrive
            // with it in every node it reaches and are checked themselves.
            _ if self.m.is_marker(t) && self.m.beside_values(t) => true,
            // DECISIONS 162: the callers' argument, stored by their units.
            _ if self.cell_flow_toks.contains(&t) && self.m.own_param_members(t).is_some() => true,
            // A global builtin is the same object in every unit (§17.16 (a)).
            TokKind::Top | TokKind::BuiltinFn | TokKind::Builtin(_) => true,
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.stand_in_cc.contains_key(&tm) => {
                !self
                    .unit_in
                    .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
            }
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.contract_class(tm) => true,
            // DECISIONS 116: an instance of a class to give.
            TokKind::Obj(tm, _) if self.class_to_give(tm) => true,
            TokKind::Obj(..) if self.arr_ok(t) => true,
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
            TokKind::Obj(tm, _) if self.keyed_leave(tm) => {
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

    /// A value a field of this unit's published literal may forward with
    /// its owner's contract (DECISIONS 88): another unit's unbroken
    /// stand-in, a builtin, an unknown value.
    fn forwarded_tok(&self, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn | TokKind::Builtin(_) => true,
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) => {
                self.stand_in_cc.contains_key(&tm)
                    && !self
                        .unit_in
                        .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
            }
            _ => self.stand_in_fc.contains_key(&t),
        }
    }

    fn sink_all_fields(&mut self, t: Tok, r: u32) {
        if self.all_sunk.insert(t) {
            let all = self.m.all_fields(t);
            self.m.cons(all, Cons::TopSink { reason: r });
        }
    }

    /// Another unit's layout a published value's field may hold as its
    /// owner publishes it (a stand-in no census broke).
    pub(crate) fn forwarded_layout(&self, tm: TmplId) -> bool {
        self.stand_in_cc.get(&tm).is_some_and(|c| c.literal_shape)
            && !self
                .unit_in
                .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
    }

    /// A literal layout of this unit a published value's field may hold
    /// as a nested contract (design §18.7): no census broke it, it is no
    /// stand-in or contract class. (A closed callee of another unit that
    /// gets it as an argument sees no nested fields; what it does to them
    /// comes back through its parameter's reach uses, decision 13.)
    fn nested_ok(&self, tm: TmplId) -> bool {
        self.literal_ok(tm) && !self.stand_in_cc.contains_key(&tm) && !self.contract_class(tm)
    }

    /// A published object whose named fields leave key by key
    /// (`nested_fields`): a literal layout of this unit (§18.7) or an
    /// instance of a contract class (DECISIONS 88: its contract describes
    /// each field as a literal's does).
    fn keyed_leave(&self, tm: TmplId) -> bool {
        self.nested_ok(tm) || self.contract_class(tm)
    }

    /// Design §18.7: the named fields of the published literals that leave
    /// key by key, decided per template and key over every instance that
    /// leaves. A field holding only instances of literal layouts this unit
    /// may publish (`nested_ok`, none escaped) is described by those
    /// layouts, which leave the same way; instances of another unit's
    /// layout no census broke (a stand-in) are described by its owner's
    /// contract, forwarded: nothing of this unit leaves through their
    /// fields (DECISIONS 79, as for a published value and a contract
    /// result, `value_tok_ok`); any other content and the field
    /// is sunk for good. A field holding only primitives so far stays
    /// undecided (nothing leaves through it; a reader sees ⊤). Re-run to
    /// the fixpoint by `contract_check`: a described field whose contents
    /// grow is checked again. True when this added constraints.
    fn nested_fields(&mut self, r: u32) -> bool {
        let mut changed = false;
        loop {
            let n0 = self.leaving.len();
            let h0 = self.handed.len();
            let mut by: FxHashMap<TmplId, Vec<Tok>> = FxHashMap::default();
            let lv: crate::pts::FxHashSet<Tok> = self.leaving.iter().copied().collect();
            for &t in self
                .leaving
                .iter()
                .chain(self.handed.iter().filter(|t| !lv.contains(t)))
            {
                if let Some(tm) = self.m.tmpl_of(t) {
                    by.entry(tm).or_default().push(t);
                }
            }
            let mut tms: Vec<TmplId> = by.keys().copied().collect();
            tms.sort_unstable();
            for tm in tms {
                let ts = by.remove(&tm).unwrap_or_default();
                // Only a leaving token's fields are sunk; a handed one's
                // (`handed`) are described or read as unknown by the callee.
                let leaves: Vec<Tok> = ts.iter().copied().filter(|t| lv.contains(t)).collect();
                let handed_only = leaves.is_empty();
                if !self.keyed_leave(tm) {
                    // Broken by the census: every field leaves as before.
                    self.field_layouts.retain(|&(t2, _), _| t2 != tm);
                    self.field_values.retain(|&(t2, _), _| t2 != tm);
                    self.field_tas.retain(|&(t2, _), _| t2 != tm);
                    for &t in &leaves {
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
                    // A hidden capture field (`__perry_cap_*`) is the class's
                    // environment, not the instance's data (DECISIONS 117,
                    // 128): reflection never shows it (the runtime's
                    // `is_class_capture_key`), and only the class's own
                    // members read it, in this unit. It does not leave.
                    if perry_hir::cap_fields::cap_field_outer_id(&self.m.atoms[a as usize])
                        .is_some()
                    {
                        continue;
                    }
                    let mut ok = !self.sunk_fields.contains(&(tm, a));
                    let mut inner: Vec<(TmplId, Tok)> = Vec::new();
                    let mut prim = false;
                    let mut fwd = false;
                    let mut arrs: Vec<Tok> = Vec::new();
                    // DECISIONS 126: this unit's typed arrays, none escaped
                    // or hard, in a field no reader reported.
                    let mut tas: Vec<Tok> = Vec::new();
                    // DECISIONS 133: instances of this unit's contract
                    // classes, none escaped.
                    let mut cls: Vec<Tok> = Vec::new();
                    let mut pbits: u16 = 0;
                    let reported = self.ta_reported.contains(&(
                        self.m.tmpls[tm as usize].display.clone(),
                        self.m.atoms[a as usize].clone(),
                    ));
                    if ok {
                        'toks: for &t in &ts {
                            let Some(&n) = self.m.fields.get(&(t, a)) else {
                                continue;
                            };
                            prim |= self.m.nodes[n as usize].prim != 0;
                            pbits |= self.m.nodes[n as usize].prim;
                            for x in self.m.nodes[n as usize].pts.to_sorted() {
                                match self.m.toks[x as usize] {
                                    TokKind::Obj(xtm, _)
                                        if !reported
                                            && matches!(
                                                self.m.tmpls[xtm as usize].kind,
                                                TmplKind::BObj {
                                                    what: "TypedArray",
                                                    ..
                                                }
                                            )
                                            && !self.stand_in_tas.iter().any(|&(_, s)| s == x)
                                            && !self.m.is_escaped(x)
                                            && !self.m.hard.contains_key(&x) =>
                                    {
                                        tas.push(x)
                                    }
                                    TokKind::Obj(itm, _)
                                        if self.nested_ok(itm) && !self.m.is_escaped(x) =>
                                    {
                                        inner.push((itm, x))
                                    }
                                    // An instance of this unit's contract
                                    // class (one a caller handed back to
                                    // its constructor, DECISIONS 133): the
                                    // field's published kind names the
                                    // class (DECISIONS 110), and the
                                    // instance leaves key by key itself.
                                    TokKind::Obj(itm, _)
                                        if self.contract_class(itm)
                                            && !self.stand_in_cc.contains_key(&itm)
                                            && !self.m.is_escaped(x) =>
                                    {
                                        cls.push(x)
                                    }
                                    // An array: its elements are a cell
                                    // (DECISIONS 94).
                                    _ if self.arr_ok(x) => {
                                        fwd = true;
                                        arrs.push(x);
                                    }
                                    // Another unit's stand-in (a builtin, an
                                    // unknown value): forwarded with its
                                    // owner's contract (DECISIONS 88).
                                    _ if self.forwarded_tok(x) => fwd = true,
                                    // DECISIONS 164: a marker with values
                                    // beside it (a published method's
                                    // receiver reach, DECISIONS 107, or an
                                    // argument marker's reach over such)
                                    // stands for no value of its own
                                    // (DECISIONS 74): the values beside it
                                    // are judged here themselves.
                                    _ if self.m.is_marker(x) && self.m.beside_values(x) => {}
                                    _ => {
                                        ok = false;
                                        break 'toks;
                                    }
                                }
                            }
                        }
                    }
                    // DECISIONS 126: only typed arrays (and `undefined`
                    // before the field is written): the owner's typed arrays,
                    // described; mixed with anything else, sunk.
                    if ok && !tas.is_empty() {
                        if fwd
                            || !inner.is_empty()
                            || !cls.is_empty()
                            || pbits & !crate::model::prim::UNDEF != 0
                        {
                            ok = false;
                        } else {
                            let mut elem = None;
                            let mut one = true;
                            for &x in &tas {
                                let k = self
                                    .m
                                    .tmpl_of(x)
                                    .and_then(|xtm| self.m.ta_elem.get(&xtm).copied());
                                match (elem, k) {
                                    (_, None) => one = false,
                                    (None, Some(k)) => elem = Some(k),
                                    (Some(e), Some(k)) if e != k => one = false,
                                    _ => {}
                                }
                            }
                            let v = (elem.filter(|_| one), pbits);
                            if self.field_tas.insert((tm, a), v) != Some(v) {
                                changed = true;
                            }
                            self.field_layouts.remove(&(tm, a));
                            self.field_values.remove(&(tm, a));
                            continue;
                        }
                    }
                    // A handed literal's instance of a contract class would
                    // leave key by key: not described here.
                    if handed_only && !cls.is_empty() {
                        ok = false;
                    }
                    if ok {
                        for x in std::mem::take(&mut cls) {
                            changed |= self.fields_leave(x, r);
                        }
                    }
                    // Forwarded stand-ins (beside this unit's layouts, which
                    // leave as nested contracts): one value contract
                    // describes the field (DECISIONS 88).
                    if ok && fwd {
                        for x in arrs {
                            changed |= self.arr_leaves(x);
                        }
                        let ns: Vec<NodeId> = ts
                            .iter()
                            .filter_map(|&t| self.m.fields.get(&(t, a)).copied())
                            .collect();
                        self.field_values.insert((tm, a), ns);
                        self.field_layouts.remove(&(tm, a));
                        for (_, x) in inner {
                            changed |= self.inner_leave(x, handed_only, r);
                        }
                        continue;
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
                            changed |= self.inner_leave(x, handed_only, r);
                        }
                        continue;
                    }
                    // Its readers find an unknown value: what it holds leaves
                    // beyond any contract (DECISIONS 203, 212), unless it
                    // holds only functions they call through their
                    // contracts (`note_field_fns`).
                    self.sunk_fields.insert((tm, a));
                    self.field_layouts.remove(&(tm, a));
                    self.field_values.remove(&(tm, a));
                    self.field_tas.remove(&(tm, a));
                    self.field_prims.remove(&(tm, a));
                    let ru = self.m.reason(UNDESCRIBED);
                    for &t in &leaves {
                        if let Some(&n) = self.m.fields.get(&(t, a)) {
                            if self.sunk_field_toks.insert((t, a)) {
                                let rf = match self.field_fns.contains_key(&(t, a)) {
                                    true => r,
                                    false => ru,
                                };
                                self.m.sink_node(n, rf);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if self.leaving.len() == n0 && self.handed.len() == h0 {
                break;
            }
        }
        changed
    }

    /// A literal of this unit a described field holds: it leaves with its
    /// holder (`fields_leave`), or is handed with it (`handed`).
    fn inner_leave(&mut self, x: Tok, handed: bool, r: u32) -> bool {
        if !handed {
            return self.fields_leave(x, r);
        }
        if !self.handed.contains(&x) {
            self.handed.push(x);
        }
        false
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
        if self.m.explain {
            eprintln!(
                "perry: wpfacts explain: unresolved scope name {name} in {}",
                self.modules[mi as usize].0
            );
        }
        self.m.top_val(&format!("unresolved scope name {name}"))
    }
}
