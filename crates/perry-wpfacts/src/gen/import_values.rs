use super::*;

impl<'a> Gen<'a> {
    /// What `import { name as local }` binds from another unit (module
    /// `path`, written `src`): the stand-in of its class, function or value
    /// contract, else ⊤ (a native, builtin or unresolved module, or an
    /// export without a contract). A name `path` re-exports is the binding
    /// it resolves to (§18.14, ECMAScript ResolveExport): the declaring
    /// module's contract, or that module's namespace.
    pub(super) fn imported_value(
        &mut self,
        mi: u32,
        path: Option<&'a str>,
        src: &'a str,
        name: &str,
        local: &str,
    ) -> NodeId {
        let unresolved = |g: &mut Self| {
            let why = format!("import from native/unresolved module {src}");
            g.m.top_val(&why)
        };
        let Some(path) = path else {
            return unresolved(self);
        };
        let imp_path = path;
        let (path, name) = match self.unit_in.and_then(|u| u.resolve(path, name)) {
            Some(crate::units::Resolved::Binding(p, n)) => (p, n),
            Some(crate::units::Resolved::Namespace(p)) => {
                return self.stand_in_ns(mi, p, p, local);
            }
            Some(crate::units::Resolved::Unknown) => {
                return self.m.top_val(&format!(
                    "export {name} of {src}: re-exported from outside the program or ambiguous"
                ));
            }
            None => (path, name),
        };
        let reexported = path != imp_path;
        if let Some(c) = self.contract_for(path, name) {
            let v = self.contract_class_value(mi, c);
            if c.binding_prim == 0 {
                return v;
            }
            // The binding's own primitives (DECISIONS 115), on this import
            // only: the stand-in is the class, shared by every use of it.
            let n = self.m.node_named("import", || format!("import({local})"));
            self.m.edge(v, n);
            self.m.add_prim(n, c.binding_prim);
            return n;
        }
        if let Some(c) = self.fn_contract_for(path, name) {
            return self.stand_in_fn(mi, c);
        }
        if let Some(c) = self.value_contract_for(path, name) {
            return self.stand_in_value(mi, local, c);
        }
        if reexported {
            // Re-exported: the declaring unit published no contract (broken
            // by the census, solved after this unit in a cycle, or none).
            let file = path.rsplit('/').next().unwrap_or(path);
            return self.m.top_val(&format!(
                "re-exported by {src}: no contract of {file} for it"
            ));
        }
        unresolved(self)
    }

    pub(super) fn ns_keys_for(&self, path: &str) -> Option<&'a crate::units::ModExports> {
        self.unit_in?.ns_keys(path)
    }

    /// Another unit's module namespace object (design §18.2): one object per
    /// imported module, its keys the module's export names, each holding
    /// what a named import of it binds; open (⊤ under any key) when an
    /// `export *` source is outside the program. A namespace's shape is its
    /// module's declarations, so nothing about it is guarded.
    pub(super) fn stand_in_ns(
        &mut self,
        mi: u32,
        path: &'a str,
        src: &'a str,
        local: &str,
    ) -> NodeId {
        if let Some(e) = self.stand_in_nss.get(path) {
            return e.node;
        }
        let open = self.ns_keys_for(path).expect("namespace keys").open;
        // Past this unit's own modules' namespaces (`ModInfo::ns_tok`).
        let id = (self.modules.len() + self.stand_in_nss.len()) as u32;
        let tok = self.m.tok(TokKind::ModNs(id));
        let n = self
            .m
            .node_named("ns-import", || format!("ns-import({local})"));
        self.m.add_tok(n, tok);
        // Its keys' values are made as the unit names them (`NsStandIn`).
        self.stand_in_nss.insert(
            path.to_string(),
            NsStandIn {
                node: n,
                tok,
                mi,
                path,
                src,
                local: local.to_string(),
                made: Default::default(),
                whole: false,
                escaped: false,
            },
        );
        if open {
            let fa = self.m.field_any(tok);
            self.m.add_top(fa, |_| {
                format!("namespace of {src}: export * from outside the program")
            });
        }
        n
    }

    /// The stand-in namespace node `n` is, by module path.
    pub(super) fn ns_path_of(&self, n: NodeId) -> Option<String> {
        self.stand_in_nss
            .iter()
            .find(|(_, e)| e.node == n)
            .map(|(p, _)| p.clone())
    }

    /// Key `k` of stand-in namespace `path`: what `import { k }` binds, held
    /// under `k`; `None` when `k` is no export (the read is `undefined`).
    pub(super) fn ns_key(&mut self, path: &str, k: &str) -> Option<NodeId> {
        let e = &self.stand_in_nss[path];
        if let Some(&v) = e.made.get(k) {
            return Some(v);
        }
        let (mi, p, src, tok) = (e.mi, e.path, e.src, e.tok);
        let local = format!("{}.{k}", e.local);
        if !self.ns_keys_for(p).is_some_and(|x| x.has(k)) {
            return None;
        }
        let v = self.at_module_scope(|g| g.imported_value(mi, Some(p), src, k, &local));
        let a = self.m.atom(k);
        let f = self.m.field(tok, a);
        self.m.edge(v, f);
        let e = self.stand_in_nss.get_mut(path).expect("namespace");
        e.made.insert(k.to_string(), v);
        // A namespace held under a key (`export * as n`, `import * as n;
        // export { n }`) is read as a value, its keys named by nothing
        // here: every key (`ns_whole`).
        self.ns_whole(v);
        Some(v)
    }

    /// Node `n` used as a value beyond reading a named key: when it is a
    /// stand-in namespace, every key of it (`NsStandIn`).
    pub(crate) fn ns_whole(&mut self, n: NodeId) {
        let Some(path) = self.ns_path_of(n) else {
            return;
        };
        let e = self.stand_in_nss.get_mut(&path).expect("namespace");
        // Set first: a namespace may reach itself through its keys.
        if std::mem::replace(&mut e.whole, true) {
            return;
        }
        let p = e.path;
        let x = self.ns_keys_for(p).expect("namespace keys");
        for (k, _) in &x.keys {
            self.ns_key(&path, k);
        }
    }

    /// Stand-in namespace `path` handed to code outside this unit: what its
    /// keys hold escapes with it, so each key whose value carries something
    /// its defining unit does not already grant unknown code is made: a
    /// class (its instances and statics may be written), a function that
    /// is closed or uses its receiver (its `fn:` report), a function whose
    /// result has layouts (their reports), a value holding such. An open
    /// function returning no layouts is run by its defining unit for any
    /// caller already (`Gen::open_fn`), and ⊤ or a primitive is what
    /// unknown code holds: such a key escapes nothing and is not made
    /// (DECISIONS 116-CWL3). Code of this unit that reads
    /// the namespace by any key still makes every key (`ns_whole`).
    pub(super) fn ns_escape(&mut self, path: &str) {
        let e = &self.stand_in_nss[path];
        if e.whole || e.escaped {
            return;
        }
        let p = e.path;
        let x = self.ns_keys_for(p).expect("namespace keys");
        for (k, _) in &x.keys {
            // The binding the key resolves to (through re-exports): its
            // declaring module holds the contract.
            let (dp, dn) = match self.unit_in.and_then(|u| u.resolve(p, k)) {
                Some(crate::units::Resolved::Binding(dp, dn)) => (dp, dn),
                Some(crate::units::Resolved::Namespace(_)) => {
                    self.ns_key(path, k);
                    continue;
                }
                Some(crate::units::Resolved::Unknown) => continue,
                None => (p, k.as_str()),
            };
            let carries = self.contract_for(dp, dn).is_some()
                || self.fn_contract_for(dp, dn).is_some_and(escape_carries)
                || self.value_contract_for(dp, dn).is_some_and(|c| {
                    !c.layouts.is_empty()
                        || !c.classes.is_empty()
                        || c.fns.iter().any(escape_carries)
                });
            if carries {
                self.ns_key(path, k);
            }
        }
        self.stand_in_nss.get_mut(path).unwrap().escaped = true;
    }

    /// `object.key` where `object` names a stand-in namespace import: the
    /// namespace node, with key `key` made (`NsStandIn`). A call
    /// `ns.key(…)` passes the namespace as its receiver. A stand-in
    /// function that ignores its receiver leaves it here; one that uses it
    /// sinks it, and an unknown callee (⊤, or no such key) lets it escape:
    /// code outside this unit gets it (`ns_escape`). Any other callee may
    /// read every key (`ns_whole`).
    pub(crate) fn ns_member(
        &mut self,
        mi: u32,
        object: &perry_hir::Expr,
        key: &str,
        call: bool,
    ) -> Option<NodeId> {
        let perry_hir::Expr::ExternFuncRef { name, .. } = object else {
            return None;
        };
        let n = self.bind_import(mi, name)?;
        let path = self.ns_path_of(n)?;
        let v = self.ns_key(&path, key);
        if call {
            let toks = v.map_or(Vec::new(), |v| self.m.nodes[v as usize].pts.to_sorted());
            let fc = |t: &Tok| self.stand_in_fc.get(t);
            let ignores =
                !toks.is_empty() && toks.iter().all(|t| fc(t).is_some_and(|c| c.lexical_this));
            let outside = v.is_none()
                || (!toks.is_empty() && toks.iter().all(|&t| t == TOP || fc(&t).is_some()));
            if !ignores && outside {
                self.ns_escape(&path);
            } else if !ignores {
                self.ns_whole(n);
            }
        }
        Some(n)
    }

    /// An import binding of module `mi` named by the unit's code: another
    /// unit's export imported by name is made now, as the bind would have
    /// made it (`unnamed_imports`); a stand-in namespace read as a value
    /// holds every key (`ns_whole`).
    pub(crate) fn import_value(&mut self, mi: u32, name: &str) -> Option<NodeId> {
        let n = self.bind_import(mi, name)?;
        self.ns_whole(n);
        Some(n)
    }

    /// The import binding `name` of module `mi`, made now if the unit's code
    /// had not named it yet, as binding the imports would have made it.
    pub(super) fn bind_import(&mut self, mi: u32, name: &str) -> Option<NodeId> {
        if let Some((imp, export)) = self.unnamed_imports.remove(&(mi, name.to_string())) {
            let p = imp.resolved_path.as_deref();
            let n = self.at_module_scope(|g| match &export {
                Some(e) => g.imported_value(mi, p, &imp.source, e, name),
                None => g.stand_in_ns(
                    mi,
                    p.expect("a namespace with keys has a path"),
                    &imp.source,
                    name,
                ),
            });
            self.mods[mi as usize].imports.insert(name.to_string(), n);
        }
        self.mods[mi as usize].imports.get(name).copied()
    }

    /// Whether `name` is an import binding of module `mi`.
    pub(crate) fn is_import(&self, mi: u32, name: &str) -> bool {
        self.mods[mi as usize].imports.contains_key(name)
            || self.unnamed_imports.contains_key(&(mi, name.to_string()))
    }

    /// `f` run where the import bindings are made: at module scope, so the
    /// values it makes belong to no function of the unit.
    pub(super) fn at_module_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.m.cur_fn, NO_FN);
        let r = f(self);
        self.m.cur_fn = outer;
        r
    }

    pub(super) fn fn_contract_for(
        &self,
        path: &str,
        name: &str,
    ) -> Option<&'a crate::units::FnContract> {
        self.unit_in?.fn_contract(path, name)
    }

    /// Another unit's function as this unit sees it through its contract
    /// (design §11.2): an argument the callee uses beyond reading and
    /// protocol touches escapes; for one it only reads or touches, those
    /// happen here to everything reachable from it (getters and protocol
    /// methods of this unit's objects run); the result is the layouts the
    /// contract names (stand-ins) or ⊤.
    pub(super) fn stand_in_fn(&mut self, mi: u32, c: &'a crate::units::FnContract) -> NodeId {
        self.stand_in_fn_of(mi, c, None)
    }

    /// DECISIONS 118: the value a contract function stores under `key`
    /// into parameter `dst`'s argument (`reach`: into what it reaches): the
    /// arguments its flow names, ⊤ and the primitives it stores (an unknown
    /// value when the contract names no flow).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stored_value(
        &mut self,
        mi: u32,
        this: NodeId,
        c: &'a crate::units::FnContract,
        params: &[NodeId],
        dst: u32,
        reach: bool,
        via: Option<&str>,
        key: Option<&str>,
    ) -> NodeId {
        let n = self.m.node_named("top", || format!("stored({})", c.name));
        self.m.own(n);
        let fl = c.param_flows.iter().find(|x| {
            x.dst == dst && x.reach == reach && x.via.as_deref() == via && x.key.as_deref() == key
        });
        let top = fl.map_or(true, |fl| fl.top);
        if let Some(fl) = fl {
            // DECISIONS 179: source `u32::MAX` is the receiver.
            let arg = |j: u32| match j {
                u32::MAX => Some(this).filter(|&t| t != crate::model::NO_NODE),
                _ => params.get(j as usize).copied(),
            };
            for &j in &fl.from {
                if let Some(a) = arg(j) {
                    self.m.edge(a, n);
                }
            }
            // DECISIONS 136: what the reader's argument reaches, under the
            // key and on from there, or through own properties.
            for (j, via) in &fl.from_reach {
                let Some(a) = arg(*j) else {
                    continue;
                };
                match via {
                    Some(k) => self.ret_through_key(a, k, &None, &[], n, &c.name),
                    None => {
                        let q = self
                            .m
                            .node_named("reach", || format!("reach-from{j}({})", c.name));
                        self.m.own(q);
                        self.m.cons(a, Cons::OwnValues { dst: q });
                        self.m.cons(q, Cons::OwnValues { dst: q });
                        self.m.edge(q, n);
                    }
                }
            }
            self.m.add_prim(n, fl.prim);
            // DECISIONS 176: the defining unit's values it stores.
            if let Some(d) = &fl.value {
                self.fill_described(
                    mi,
                    c,
                    d,
                    &d.value,
                    this,
                    params,
                    n,
                    "stored by another unit's function",
                );
            }
        }
        if top {
            self.m
                .add_top(n, |_| "stored by another unit's function".into());
            self.m.add_prim(n, prim::ALL);
        }
        n
    }

    /// Node `out` holds what value `d` of contract `c` may be (DECISIONS
    /// 140): its result, or an argument it passes a function of this unit.
    /// The receiver or an argument itself, what it holds under a key or
    /// reaches (unless the contract sinks it: then nothing of it comes
    /// back), the builtins, program objects and arrays (`value`: `d`'s,
    /// the contract's own for its result), the layouts.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fill_described(
        &mut self,
        mi: u32,
        c: &crate::units::FnContract,
        d: &crate::units::Described,
        value: &'a crate::units::ValueContract,
        this: NodeId,
        params: &[NodeId],
        out: NodeId,
        why: &'static str,
    ) {
        use crate::model::MARK_ESCAPE;
        if !c.lexical_this && c.this_use & MARK_ESCAPE == 0 {
            if d.params.contains(&u32::MAX) {
                self.m.edge(this, out);
            }
            for (_, k, then, steps) in d.reach_keys.iter().filter(|r| r.0 == u32::MAX) {
                self.ret_through_key(this, k, then, steps, out, &c.name);
            }
            if d.reach.contains(&u32::MAX) {
                self.reach_into(this, out, "reach-this", &c.name);
            }
        }
        for (i, &p) in params.iter().enumerate() {
            if c.params[i] & MARK_ESCAPE != 0 {
                continue;
            }
            if d.params.contains(&(i as u32)) {
                self.m.edge(p, out);
            }
            for (_, k, then, steps) in d.reach_keys.iter().filter(|r| r.0 == i as u32) {
                self.ret_through_key(p, k, then, steps, out, &c.name);
            }
            if d.reach.contains(&(i as u32)) {
                self.reach_into(p, out, "reach", &c.name);
            }
        }
        // Its primitive kinds are the defining unit's (§18.7), any when ⊤.
        self.m.add_prim(
            out,
            match d.layouts {
                Some(_) => d.prim,
                None => prim::ALL,
            },
        );
        if d.unknown {
            self.m.add_top(out, |_| why.into());
        }
        // DECISIONS 94: the builtins and arrays it holds; DECISIONS 147: the
        // functions.
        if !(value.builtins.is_empty()
            && value.arrays.is_empty()
            && value.boxes.is_empty()
            && value.fns.is_empty())
        {
            let v = self.stand_in_value(mi, why, value);
            self.m.edge(v, out);
        }
        match &d.layouts {
            Some(ds) => {
                for x in ds {
                    // A broken layout is ⊤ to every reader (§11.3).
                    match self.other_layout(x) {
                        Some(cc) => {
                            self.stand_in_class(mi, cc);
                            let tm = self.stand_ins[&cc.display].1;
                            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                            self.m.add_tok(out, inst);
                        }
                        None => self.m.add_top(out, |_| why.into()),
                    }
                }
            }
            None => self.m.add_top(out, |_| why.into()),
        }
    }

    /// `out` holds everything `holder` reaches through own and inherited
    /// fields, transitively (a contract's `ret_reach`).
    pub(super) fn reach_into(&mut self, holder: NodeId, out: NodeId, label: &str, name: &str) {
        let q = self.m.node_named("reach", || format!("{label}({name})"));
        self.m.own(q);
        self.m.cons(
            holder,
            Cons::Load {
                key: crate::model::Key::Any,
                dst: q,
            },
        );
        self.m.cons(
            q,
            Cons::Load {
                key: crate::model::Key::Any,
                dst: q,
            },
        );
        self.m.edge(q, out);
    }

    /// A contract result through `k` of `holder` (`ret_reach_keys`): the
    /// value of `holder.k` and what it reaches through `then` repeatedly
    /// (`None`: any key, DECISIONS 138), then each further step of the
    /// path with the keys repeated there (DECISIONS 158).
    pub(super) fn ret_through_key(
        &mut self,
        holder: NodeId,
        k: &str,
        then: &Option<Vec<String>>,
        steps: &[(Option<String>, Option<Vec<String>>)],
        ret: NodeId,
        name: &str,
    ) {
        let q = self.m.node_named("reach", || format!("reach.{k}({name})"));
        self.m.own(q);
        let a = self.m.atom(k);
        self.m.cons(
            holder,
            Cons::Load {
                key: crate::model::Key::Named(a),
                dst: q,
            },
        );
        if !steps.is_empty() {
            // The values at the path: each step loads its key from the
            // values of the step before, then repeats its loop keys.
            let mut cur = q;
            for (j, (sk, l)) in std::iter::once((&Some(k.to_string()), then))
                .chain(steps.iter().map(|(sk, l)| (sk, l)))
                .enumerate()
            {
                let n = match j {
                    0 => q,
                    _ => {
                        let n = self
                            .m
                            .node_named("reach", || format!("reach.{k}.{j}({name})"));
                        self.m.own(n);
                        let key = match sk {
                            Some(s) => crate::model::Key::Named(self.m.atom(s)),
                            None => crate::model::Key::Any,
                        };
                        self.m.cons(cur, Cons::Load { key, dst: n });
                        n
                    }
                };
                let keys: Vec<crate::model::Key> = match l {
                    None => vec![crate::model::Key::Any],
                    Some(ks) => ks
                        .iter()
                        .map(|s| crate::model::Key::Named(self.m.atom(s)))
                        .collect(),
                };
                for key in keys {
                    self.m.cons(n, Cons::Load { key, dst: n });
                }
                cur = n;
            }
            self.m.edge(cur, ret);
            return;
        }
        self.m.edge(q, ret);
        let keys: Vec<crate::model::Key> = match then {
            None => vec![crate::model::Key::Any],
            Some(ks) if ks.is_empty() => return,
            Some(ks) => ks
                .iter()
                .map(|s| crate::model::Key::Named(self.m.atom(s)))
                .collect(),
        };
        let r = self.m.node_named("reach", || format!("reach.{k}*({name})"));
        self.m.own(r);
        for key in keys {
            self.m.cons(q, Cons::Load { key, dst: r });
            self.m.cons(r, Cons::Load { key, dst: r });
        }
        self.m.edge(r, ret);
    }

    /// `stand_in_fn` for the function a field of stand-in template `holder`
    /// holds: called as a method, its receiver is that object, which the
    /// defining unit runs it on (`note_field_fns`); any other receiver
    /// escapes. An arrow function ignores its receiver. A static member
    /// (`holder.1`) is called on the class object instead.
    pub(super) fn stand_in_fn_of(
        &mut self,
        mi: u32,
        c: &'a crate::units::FnContract,
        holder: Option<(TmplId, bool)>,
    ) -> NodeId {
        use crate::model::{MARK_ESCAPE, MARK_TOUCH};
        let c = self.owner_fn(c);
        if let Some(&(n, _)) = self.stand_in_fns.get(&(c.name.clone(), holder)) {
            return n;
        }
        let r = self.m.reason("passed to another unit's function");
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        let outer_fn = std::mem::replace(&mut self.m.cur_fn, fx);
        let this = self.m.node_named("this", || format!("this({})", c.name));
        let ret = self.m.node_named("ret", || format!("ret({})", c.name));
        let args_all = self.m.node_named("args", || format!("args({})", c.name));
        let params: Vec<NodeId> = if c.variadic {
            Vec::new()
        } else {
            (0..c.params.len())
                .map(|i| {
                    self.m
                        .node_named("param", || format!("param{i}({})", c.name))
                })
                .collect()
        };
        self.m.own(ret);
        self.m.own(this);
        for &p in &params {
            self.m.own(p);
        }
        // The rest array of the callee's body (§18.11): the arguments past
        // its parameters land in it. The contract does not describe what
        // the body does with it, so it escapes, and its elements with it.
        let rest = c.rest.then(|| {
            let n = self.m.node_named("rest", || format!("rest({})", c.name));
            self.m.own(n);
            let arr = self.new_arr_template(mi, "rest");
            let at = self.m.tok(TokKind::Obj(arr, NO_CTX));
            self.m.add_tok(n, at);
            n
        });
        self.m.cur_fn = outer_fn;
        // One body for every call, written by the calls alone: a run whose
        // parameters are relative (DECISIONS 44 (b)).
        let run = crate::model::FN_RUN | fx;
        for &p in &params {
            self.m.nodes[p as usize].frame = run;
            self.m.nodes[p as usize].rel = true;
        }
        for n in [this, ret] {
            self.m.nodes[n as usize].frame = run;
        }
        self.m.nodes[ret as usize].result = true;
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: c.name.clone(),
            params: params.clone(),
            rest,
            this,
            ret,
            args_all,
            has_arguments: c.variadic,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_step: false,
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded: c.closed.is_none(),
        });
        // Registered before its result is built: a result layout whose
        // field holds this function reaches it again (`stand_in_class`).
        let n = self.m.node_named("fnval", || format!("fnval({})", c.name));
        self.m.add_tok(n, tok);
        self.stand_in_fns.insert((c.name.clone(), holder), (n, tok));
        self.stand_in_fc.insert(tok, c);
        // What unknown code receives goes nowhere here.
        self.m.nodes[args_all as usize].sink = true;
        self.m.sink_node(args_all, r);
        if let Some(n) = rest {
            self.m.sink_node(n, r);
        }
        match holder {
            _ if c.lexical_this => {}
            Some((tm, statics)) => {
                // Its receiver contract (DECISIONS 57): a subclass instance
                // may run it.
                let tc = (!statics).then(|| {
                    // DECISIONS 179: what it stores under each key.
                    let values = c
                        .this_stores
                        .iter()
                        .flatten()
                        .map(|k| {
                            self.stored_value(mi, this, c, &params, u32::MAX, false, None, Some(k))
                        })
                        .collect();
                    self.m.this_contracts.push(crate::model::ThisContract {
                        uses: c.this_use,
                        reach: c.this_reach,
                        stores: c.this_stores.clone(),
                        values,
                    });
                    self.m.this_contract_fns.push(tok);
                    self.m.this_contracts.len() as u32 - 1
                });
                self.m.cons(
                    this,
                    Cons::ContractThis {
                        tmpl: tm,
                        reason: r,
                        statics,
                        this: tc.unwrap_or(u32::MAX),
                    },
                );
                // The calls of keys only a subclass of another unit defines
                // (DECISIONS 97): the method's unit could not run them, so
                // each receiver runs its own chain's method here, with
                // unknown arguments; the result goes back to that unit.
                if !statics {
                    for k in &c.this_calls {
                        let a = self.m.atom(k);
                        let arg = self.m.node_l("override-arg");
                        self.m
                            .add_top(arg, |_| "argument of another unit's method".to_string());
                        self.m.add_prim(arg, crate::model::prim::ALL);
                        let res =
                            self.mcall(this, crate::model::Key::Named(a), vec![arg], Vec::new());
                        self.m.sink_node(res, r);
                    }
                }
            }
            None => self.m.sink_node(this, r),
        }
        for (i, &p) in params.iter().enumerate() {
            let u = c.params[i];
            let ru = c.reach[i];
            if u & MARK_ESCAPE != 0 {
                self.m.sink_node(p, r);
                continue;
            }
            if u & MARK_TOUCH != 0 {
                self.m.cons(
                    p,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::Any,
                    },
                );
            }
            // The keys the callee stores into the argument (DECISIONS 84):
            // what it stores under each (DECISIONS 118), a store this unit
            // performs.
            match c.param_stores.get(i) {
                Some(Some(ks)) if ks.is_empty() => {}
                None => {}
                Some(Some(ks)) => {
                    let site = self.m.anon_post_site("store by another unit's function");
                    for k in ks {
                        let src =
                            self.stored_value(mi, this, c, &params, i as u32, false, None, Some(k));
                        let key = crate::model::Key::Named(self.m.atom(k));
                        self.m.cons(p, Cons::Store { key, src, site });
                    }
                }
                ks => {
                    let top = self.m.node_named("top", || format!("stored({})", c.name));
                    self.m.own(top);
                    self.m
                        .add_top(top, |_| "stored by another unit's function".into());
                    self.m.add_prim(top, prim::ALL);
                    let site = self.m.anon_post_site("store by another unit's function");
                    let keys: Vec<crate::model::Key> = match ks {
                        Some(Some(ks)) => ks
                            .iter()
                            .map(|k| crate::model::Key::Named(self.m.atom(k)))
                            .collect(),
                        _ => vec![crate::model::Key::Any],
                    };
                    for key in keys {
                        self.m.cons(
                            p,
                            Cons::Store {
                                key,
                                src: top,
                                site,
                            },
                        );
                    }
                }
            }
            // DECISIONS 162: the array cells the callee stores the argument
            // into: this unit's store into its view of each (another unit's
            // array is a stand-in sharing the cell; its own array leaves by
            // the same cell). The cell check then describes the argument.
            for (_, key) in c.param_cells.iter().filter(|(pi, _)| *pi as usize == i) {
                // DECISIONS 184: `box:` names a primitive method container.
                let (name, boxed) = match (key.strip_prefix("arr:"), key.strip_prefix("box:")) {
                    (Some(n), _) => (n, false),
                    (_, Some(n)) => (n, true),
                    _ => continue,
                };
                let a = match self.arr_cells.iter().find(|(k, _)| k == key) {
                    Some(&(_, t)) => t,
                    None => self.stand_in_arr(mi, name, boxed),
                };
                let an = self.m.node_named("cellarr", || format!("cellarr({key})"));
                self.m.own(an);
                self.m.add_tok(an, a);
                let site = self.m.anon_post_site("store by another unit's function");
                self.m.cons(
                    an,
                    Cons::Store {
                        key: crate::model::Key::Any,
                        src: p,
                        site,
                    },
                );
            }
            // What the argument reaches, used apart from the argument
            // (DECISIONS 45). An escape is transitive (`escape` sinks every
            // field): the argument's fields go to the unit's escape node. A
            // touch is not: the unit's touch node closes over the fields of
            // what it holds. The union of the parameters' closures is the
            // closure of their union, so one node per use holds the same
            // facts as one per parameter. A read only runs nothing here.
            // Per key the callee read it under (§18.4): what `p.k` reaches
            // is the argument's `k`; a computed key reaches all of it.
            let mut buckets = vec![(crate::model::Key::Any, ru)];
            for (k, u) in c.reach_keys.get(i).into_iter().flatten() {
                buckets.push((crate::model::Key::Named(self.m.atom(k)), *u));
            }
            for (key, ru) in buckets {
                if ru & MARK_ESCAPE != 0 {
                    let q = match self.reach_escape {
                        Some(q) => q,
                        None => {
                            let q = self.m.node_named("reach", || "reached(escape)".into());
                            self.m.nodes[q as usize].sink = true;
                            self.m.sink_node(q, r);
                            self.reach_escape = Some(q);
                            q
                        }
                    };
                    self.m.cons(p, Cons::Load { key, dst: q });
                } else if ru & MARK_TOUCH != 0 {
                    let q = match self.reach_touch {
                        Some(q) => q,
                        None => {
                            let q = self.m.node_named("reach", || "reached(touch)".into());
                            self.m.nodes[q as usize].sink = true;
                            self.m.cons(
                                q,
                                Cons::Load {
                                    key: crate::model::Key::Any,
                                    dst: q,
                                },
                            );
                            self.m.cons(
                                q,
                                Cons::Implicit {
                                    reason: r,
                                    proto: Protocol::Any,
                                },
                            );
                            self.reach_touch = Some(q);
                            q
                        }
                    };
                    self.m.cons(p, Cons::Load { key, dst: q });
                }
            }
        }
        // DECISIONS 118: what the callee stores into what an argument
        // reaches: under the key of what the argument holds under `via`
        // (its keyed reach), else of every object the argument reaches
        // through own properties (not the argument itself: a store into it
        // is a `param_stores` one). Like the callee's reach marker, which
        // has no prototype chain (DECISIONS 119), the closure follows no
        // prototype: an intrinsic is no value the argument reaches.
        // DECISIONS 149: into what the receiver reaches too (the instances
        // `this` admits), and under a computed key.
        for fl in c.param_flows.iter().filter(|x| x.reach) {
            let p = match fl.dst {
                u32::MAX => this,
                d => match params.get(d as usize) {
                    Some(&p) => p,
                    None => continue,
                },
            };
            let q = self
                .m
                .node_named("reach", || format!("reach-stored{}({})", fl.dst, c.name));
            self.m.own(q);
            match &fl.via {
                Some(v) => {
                    let key = crate::model::Key::Named(self.m.atom(v));
                    self.m.cons(p, Cons::Load { key, dst: q });
                }
                None => {
                    self.m.cons(p, Cons::OwnValues { dst: q });
                    self.m.cons(q, Cons::OwnValues { dst: q });
                }
            }
            let src = self.stored_value(
                mi,
                this,
                c,
                &params,
                fl.dst,
                true,
                fl.via.as_deref(),
                fl.key.as_deref(),
            );
            let site = self.m.anon_post_site("store by another unit's function");
            let key = match &fl.key {
                Some(k) => crate::model::Key::Named(self.m.atom(k)),
                None => crate::model::Key::Any,
            };
            self.m.cons(q, Cons::Store { key, src, site });
        }
        // What it returns, as the defining unit described it.
        let res = crate::units::Described {
            layouts: c.ret.clone(),
            params: c.ret_params.clone(),
            reach: c.ret_reach.clone(),
            reach_keys: c.ret_reach_keys.clone(),
            unknown: c.ret_unknown,
            value: Default::default(),
            prim: c.ret_prim,
        };
        let why = "result of another unit's function";
        self.fill_described(mi, c, &res, &c.ret_value, this, &params, ret, why);
        // DECISIONS 140: the calls its body makes through a parameter. The
        // defining unit described their arguments; this unit runs each call
        // on what it passes (the function is this unit's, or one it holds a
        // contract of; DECISIONS 144: a method call runs that method of
        // it). What they return goes to the defining unit, which reads it as
        // unknown: it leaves.
        for pc in &c.param_calls {
            let i = pc.param as usize;
            let Some(&p) = params.get(i) else {
                continue;
            };
            if c.params[i] & MARK_ESCAPE != 0 {
                continue;
            }
            let mut args = Vec::with_capacity(pc.args.len());
            for (j, d) in pc.args.iter().enumerate() {
                let a = self
                    .m
                    .node_named("arg", || format!("called{i}-arg{j}({})", c.name));
                self.m.own(a);
                let why = "argument from another unit's function";
                self.fill_described(mi, c, d, &d.value, this, &params, a, why);
                args.push(a);
            }
            let res = match &pc.key {
                None => self.call_value(p, ThisArg::Undef, args, Vec::new()),
                Some(k) => {
                    let key = Key::Named(self.m.atom(k));
                    self.mcall(p, key, args, Vec::new())
                }
            };
            self.m.sink_node(res, r);
        }
        // DECISIONS 152: the arguments the body hands to a closed class's
        // constructor: this unit holds them, so it passes them there as its
        // own construction would (the constructor's parameters publish what
        // they hold, `stand_in_class`). No contract of the class: they leave.
        for ho in &c.param_handovers {
            let i = ho.param as usize;
            let Some(&p) = params.get(i) else {
                continue;
            };
            if c.params[i] & MARK_ESCAPE != 0 {
                continue;
            }
            let cc = self
                .unit_in
                .and_then(|u| u.closed_class(&ho.class.0, &ho.class.1));
            let tm = cc.and_then(|cc| match self.own_class(&cc.display) {
                Some(tm) => Some(tm),
                None => {
                    self.stand_in_class(mi, cc);
                    self.stand_ins.get(&cc.display).map(|e| e.1)
                }
            });
            let q = tm.and_then(|tm| self.m.tmpls[tm as usize].ctor).map(|f| {
                self.m.funcs[f as usize]
                    .params
                    .get(ho.arg as usize)
                    .copied()
            });
            match q {
                Some(Some(q)) => self.m.edge(p, q),
                // Past its parameters: reaches nothing (`lib::closed_of`).
                Some(None) => {}
                None => self.m.sink_node(p, r),
            }
        }
        n
    }
}
