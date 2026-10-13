//! Constraint generation, part 1: program setup (modules, classes,
//! functions, imports/exports) and statements. Expressions live in
//! `gen_expr.rs`; the exhaustive variant list in `gen_rest.rs`.

use crate::ctor_phase::CtorPhase;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::{Class, Export, Function, ImportSpecifier, Module, Stmt};

/// A read site the precision report can match against the dynamic census:
/// `recv.key` with a source byte offset.
#[derive(Clone, Debug)]
pub struct ReadSite {
    pub module: u32,
    pub byte_offset: u32,
    pub key: Atom,
    pub recv: NodeId,
    pub is_call: bool,
    pub func: FnIdx,
}

pub(crate) struct ModInfo {
    pub path: String,
    pub locals: FxHashMap<u32, NodeId>,
    pub globals: FxHashMap<u32, NodeId>,
    pub funcs: FxHashMap<u32, FnIdx>,
    pub classes: FxHashMap<String, TmplId>,
    /// import local name → binding node
    pub imports: FxHashMap<String, NodeId>,
    /// module-scope names → value node (init `Let`s, functions, classes)
    pub scope: FxHashMap<String, NodeId>,
    pub ns_tok: Tok,
    pub site_counter: u32,
    pub iterres: FxHashMap<FnIdx, NodeId>,
    pub iterout: FxHashMap<FnIdx, NodeId>,
}

pub struct Gen<'a> {
    pub m: Model,
    pub modules: Vec<(&'a str, &'a Module)>,
    pub(crate) mods: Vec<ModInfo>,
    pub path_index: FxHashMap<String, u32>,
    pub exports: FxHashMap<(u32, Atom), NodeId>,
    pub export_resolving: crate::pts::FxHashSet<(u32, Atom)>,
    pub thrown: NodeId,
    pub cjs_registry: NodeId,
    pub read_sites: Vec<ReadSite>,
    /// E14: the program may run interpreted code (non-constant
    /// `new Function` / `eval` / `vm`): such code can patch
    /// `Object.prototype`, so no template is prefix-stable.
    pub global_hard: Option<String>,
    pub ctor_phase: FxHashMap<TmplId, CtorPhase>,
    pub fn_inst_of: FxHashMap<FnIdx, TmplId>,
    pub field_init_fn: FxHashMap<TmplId, FnIdx>,
    pub static_init_fn: FxHashMap<TmplId, FnIdx>,
    pub prim_const: [NodeId; 7],
    pub walked: crate::pts::FxHashSet<FnIdx>,
    pub class_envs: FxHashMap<String, NodeId>,
    /// Object-literal IIFE parameter node → the literal template it births.
    pub iife_birth: FxHashMap<NodeId, TmplId>,
    /// Construction-phase analysis of plain functions used as constructors.
    pub fn_phase: FxHashMap<FnIdx, CtorPhase>,
    /// Locals holding a class method's lexical home (`ClassMethodHome`):
    /// (module, local) → the class. `super` through such a home starts at
    /// that class's parent, exactly like `SuperPropertyGet`.
    pub class_homes: FxHashMap<(u32, u32), TmplId>,
}

/// Per-function walking context.
#[derive(Clone)]
pub(crate) struct Fx {
    pub module: u32,
    pub f: FnIdx,
    pub this: NodeId,
    pub ret: NodeId,
    /// Class whose method/ctor this is (for `super`).
    pub class: Option<TmplId>,
    /// Constructor of this class template (construction-phase tracking).
    pub ctor_of: Option<TmplId>,
    pub new_target: NodeId,
    /// A plain function / function expression that `new` may call.
    pub fn_ctor: bool,
}

impl<'a> Gen<'a> {
    pub fn new(modules: Vec<(&'a str, &'a Module)>, relations: bool, explain: bool) -> Self {
        let mut m = Model::default();
        m.explain = explain;
        m.cur_src = NO_NODE;
        m.cur_why = u32::MAX;
        m.rel_phase = b'G';
        if relations {
            m.relations = Some(Vec::new());
        }
        let top = m.tok(TokKind::Top);
        debug_assert_eq!(top, TOP);
        m.top_node = m.node_l("TOPNODE");
        let top_tok_node = m.node_l("TOPVAL");
        m.add_tok(top_tok_node, TOP);
        m.add_prim(top_tok_node, prim::ALL);
        m.top_tok_node = top_tok_node;
        let mut prim_const = [0; 7];
        for (i, p) in [
            prim::NUM,
            prim::STR,
            prim::BOOL,
            prim::NULL,
            prim::UNDEF,
            prim::BIGINT,
            prim::SYMBOL,
        ]
        .into_iter()
        .enumerate()
        {
            let n = m.node_named("PRIM", || format!("PRIM{p}"));
            m.add_prim(n, p);
            prim_const[i] = n;
        }
        let all = m.node_l("PRIMALL");
        m.add_prim(all, prim::ALL);
        m.all_prim_node = all;
        let thrown = m.node_l("THROWN");
        // Runtime-thrown errors are runtime objects: ⊤-origin.
        m.add_top(thrown, |_| "thrown value (runtime errors)".into());
        m.add_prim(thrown, prim::ALL);
        m.thrown = thrown;
        let cjs_registry = m.node_l("CJSREG");
        let _ = m.tok(TokKind::ObjectProto);
        let mut path_index = FxHashMap::default();
        for (i, (p, _)) in modules.iter().enumerate() {
            path_index.insert(p.to_string(), i as u32);
        }
        Gen {
            m,
            modules,
            mods: Vec::new(),
            path_index,
            exports: FxHashMap::default(),
            export_resolving: Default::default(),
            thrown,
            cjs_registry,
            read_sites: Vec::new(),
            global_hard: None,
            ctor_phase: FxHashMap::default(),
            fn_inst_of: FxHashMap::default(),
            field_init_fn: FxHashMap::default(),
            static_init_fn: FxHashMap::default(),
            prim_const,
            walked: Default::default(),
            class_envs: FxHashMap::default(),
            iife_birth: FxHashMap::default(),
            fn_phase: FxHashMap::default(),
            class_homes: FxHashMap::default(),
        }
    }

    pub fn prim_node(&self, p: u16) -> NodeId {
        match p {
            prim::NUM => self.prim_const[0],
            prim::STR => self.prim_const[1],
            prim::BOOL => self.prim_const[2],
            prim::NULL => self.prim_const[3],
            prim::UNDEF => self.prim_const[4],
            prim::BIGINT => self.prim_const[5],
            prim::SYMBOL => self.prim_const[6],
            _ => self.m.all_prim_node,
        }
    }

    // ------------------------------------------------------------ registry

    /// Register every module's functions, classes and scope before any body
    /// is walked, so cross-references resolve in one pass.
    pub fn register_all(&mut self) {
        let modules = self.modules.clone();
        for (mi, (path, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            let ns_tok = self.m.tok(TokKind::ModNs(mi));
            self.mods.push(ModInfo {
                path: path.to_string(),
                locals: FxHashMap::default(),
                globals: FxHashMap::default(),
                funcs: FxHashMap::default(),
                classes: FxHashMap::default(),
                imports: FxHashMap::default(),
                scope: FxHashMap::default(),
                ns_tok,
                site_counter: 0,
                iterres: FxHashMap::default(),
                iterout: FxHashMap::default(),
            });
            for f in &module.functions {
                self.register_fn(mi, f.id, &f.name, &f.params, false, false);
            }
            for c in &module.classes {
                self.register_class(mi, c);
            }
            // Closures are registered when the walker meets them.
            let _ = module;
        }
        // Class parents and native parents (second pass: names resolve).
        for (mi, (_, module)) in modules.iter().enumerate() {
            for c in &module.classes {
                self.link_class(mi as u32, c);
            }
        }
        // Module scope names (top-level Lets, functions, classes).
        for (mi, (_, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            for s in &module.init {
                if let Stmt::Let { id, name, .. } = s {
                    let n = self.local(mi, *id);
                    self.mods[mi as usize].scope.insert(name.clone(), n);
                }
            }
            for f in &module.functions {
                let fx = self.mods[mi as usize].funcs[&f.id];
                let tok = self.m.funcs[fx as usize].token;
                let n = self.m.node_named("fnval", || format!("fnval({})", f.name));
                self.m.add_tok(n, tok);
                self.mods[mi as usize]
                    .scope
                    .entry(f.name.clone())
                    .or_insert(n);
            }
            for c in &module.classes {
                let tm = self.mods[mi as usize].classes[&c.name];
                let tok = self.m.tok(TokKind::ClassObj(tm));
                let n = self
                    .m
                    .node_named("classval", || format!("classval({})", c.name));
                self.m.add_tok(n, tok);
                self.mods[mi as usize]
                    .scope
                    .entry(c.name.clone())
                    .or_insert(n);
                for a in &c.aliases {
                    self.mods[mi as usize].scope.entry(a.clone()).or_insert(n);
                }
            }
            for g in &module.globals {
                let n = self.global(mi, g.id);
                self.mods[mi as usize]
                    .scope
                    .entry(g.name.clone())
                    .or_insert(n);
            }
        }
        // Imports bind to the exporting module's export nodes.
        for (mi, (_, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            for imp in &module.imports {
                if imp.type_only {
                    continue;
                }
                let target = imp
                    .resolved_path
                    .as_ref()
                    .and_then(|p| self.path_index.get(p).copied());
                for spec in &imp.specifiers {
                    let (local, node) = match (spec, target) {
                        (ImportSpecifier::Named { imported, local }, Some(t)) => {
                            let a = self.m.atom(imported);
                            (local.clone(), self.export_node(t, a))
                        }
                        (ImportSpecifier::Default { local }, Some(t)) => {
                            let a = self.m.atom("default");
                            (local.clone(), self.export_node(t, a))
                        }
                        (ImportSpecifier::Namespace { local }, Some(t)) => {
                            let tok = self.mods[t as usize].ns_tok;
                            let n = self
                                .m
                                .node_named("ns-import", || format!("ns-import({local})"));
                            self.m.add_tok(n, tok);
                            (local.clone(), n)
                        }
                        // Native / builtin / unresolved modules: ⊤ values.
                        (ImportSpecifier::Named { local, .. }, None)
                        | (ImportSpecifier::Default { local }, None)
                        | (ImportSpecifier::Namespace { local }, None) => {
                            let why = format!("import from native/unresolved module {}", imp.source);
                            (local.clone(), self.m.top_val(&why))
                        }
                    };
                    self.mods[mi as usize].imports.insert(local, node);
                }
            }
        }
    }

    pub(crate) fn register_fn(
        &mut self,
        mi: u32,
        id: u32,
        name: &str,
        params: &[perry_hir::Param],
        lexical_this: bool,
        _is_closure: bool,
    ) -> FnIdx {
        if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
            return f;
        }
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx));
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        let mut ps = Vec::new();
        let mut rest = None;
        let mut mapped = None;
        for p in params {
            let n = self.local(mi, p.id);
            if p.is_rest {
                // The rest local holds a fresh array whose elements are the
                // extra arguments (and, for `arguments`, every argument).
                let arr = self.new_arr_template(mi, "rest");
                let at = self.m.tok(TokKind::Obj(arr));
                self.m.add_tok(n, at);
                let fa = self.m.field_any(at);
                if p.arguments_object.is_some() {
                    self.m.edge(args_all, fa);
                    // Sloppy mapped `arguments` aliases the params: writes
                    // through it reach them (E24). The synthetic parameter
                    // may come before the named ones, so the edges are added
                    // once every parameter is known.
                    if p.arguments_object.as_ref().is_some_and(|a| !a.strict) {
                        mapped = Some((fa, self.m.all_fields(at)));
                    }
                }
                rest = Some(n);
            } else {
                ps.push(n);
            }
        }
        if let Some((fa, all)) = mapped {
            // Both directions: a write to `arguments[i]` reaches the
            // parameter, and an assignment to the parameter is read back
            // through `arguments[i]`. A write under a constant index lands
            // in that index's field, so the write direction reads every
            // field of the arguments object; the read direction feeds the
            // any-field node, which reaches every named field.
            for &q in &ps {
                self.m.edge(all, q);
                self.m.edge(q, fa);
            }
        }
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: id,
            name: format!("{}:{}", self.mods[mi as usize].path_short(), name),
            params: ps,
            rest,
            this,
            ret,
            args_all,
            token: tok,
            ctor_of: None,
            lexical_this,
        });
        self.mods[mi as usize].funcs.insert(id, fx);
        fx
    }

    fn register_class(&mut self, mi: u32, c: &Class) {
        let tm = self.m.tmpls.len() as TmplId;
        let display = format!("{}:{}", self.mods[mi as usize].path_short(), c.name);
        let mut accessor_keys = Vec::new();
        for (n, _) in c.getters.iter().chain(c.setters.iter()) {
            let a = self.m.atom(n);
            if !accessor_keys.contains(&a) {
                accessor_keys.push(a);
            }
        }
        let proto_members = {
            let mut v = accessor_keys.clone();
            for meth in &c.methods {
                let a = self.m.atom(&meth.name);
                push_unique(&mut v, a);
            }
            let a = self.m.atom("constructor");
            push_unique(&mut v, a);
            v
        };
        self.m.tmpls.push(Template {
            kind: TmplKind::Class {
                module: mi,
                name: c.name.clone(),
                class_id: c.id,
                literal_shape: c.is_literal_shape(),
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: c.native_extends.clone(),
            accessor_keys: accessor_keys.clone(),
            has_computed_members: !c.computed_members.is_empty(),
            field_init: None,
            proto_members,
        });
        self.mods[mi as usize].classes.insert(c.name.clone(), tm);
        for a in &c.aliases {
            self.mods[mi as usize]
                .classes
                .entry(a.clone())
                .or_insert(tm);
        }
        // Functions of the class.
        if let Some(ctor) = &c.constructor {
            let f = self.register_fn(mi, ctor.id, &ctor.name, &ctor.params, false, false);
            self.m.funcs[f as usize].ctor_of = Some(tm);
            self.m.tmpls[tm as usize].ctor = Some(f);
        }
        let proto = self.m.tok(TokKind::Proto(tm));
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        let pa = self.m.atom("prototype");
        let pf = self.m.field(cobj, pa);
        self.m.add_tok(pf, proto);
        let ca = self.m.atom("constructor");
        let cf = self.m.field(proto, ca);
        self.m.add_tok(cf, cobj);
        for meth in &c.methods {
            let f = self.register_fn(mi, meth.id, &meth.name, &meth.params, false, false);
            let a = self.m.atom(&meth.name);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
        }
        for meth in &c.static_methods {
            let f = self.register_fn(mi, meth.id, &meth.name, &meth.params, false, false);
            let a = self.m.atom(&meth.name);
            let n = self.m.field(cobj, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
        }
        for (name, g) in &c.getters {
            let f = self.register_fn(mi, g.id, &g.name, &g.params, false, false);
            let a = self.m.atom(name);
            self.m.accessor_atoms.insert(a);
            self.m.accessors.entry((proto, a)).or_default().0.push(f);
            push_unique(self.m.accessor_keys.entry(proto).or_default(), a);
        }
        for (name, s) in &c.setters {
            let f = self.register_fn(mi, s.id, &s.name, &s.params, false, false);
            let a = self.m.atom(name);
            self.m.accessor_atoms.insert(a);
            self.m.accessors.entry((proto, a)).or_default().1.push(f);
            push_unique(self.m.accessor_keys.entry(proto).or_default(), a);
        }
        for cm in &c.computed_members {
            let f = self.register_fn(
                mi,
                cm.function.id,
                &cm.function.name,
                &cm.function.params,
                false,
                false,
            );
            let holder = if cm.is_static { cobj } else { proto };
            let any = self.m.field_any(holder);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(any, t);
            // A computed accessor may be named after any key.
            if !matches!(cm.kind, perry_hir::ClassComputedMemberKind::Method) {
                // Conservative: treat as a getter/setter on every key by
                // making every read through the prototype call it.
                let this = self.m.funcs[f as usize].this;
                let pn = self.m.node_l("computed-accessor-this");
                let _ = pn;
                // Instances reaching any key read: approximate by making
                // the accessor's `this` the whole class (instances).
                let inst = self.m.tok(TokKind::Obj(tm));
                self.m.add_tok(this, inst);
                let fa = self.m.field_any(proto);
                let ret = self.m.funcs[f as usize].ret;
                self.m.edge(ret, fa);
            }
        }
        // Synthetic field-initializer and static-initializer contexts.
        let fi = self.synthetic_fn(mi, &format!("{}::<fields>", c.name));
        self.field_init_fn.insert(tm, fi);
        self.m.tmpls[tm as usize].field_init = Some(fi);
        let si = self.synthetic_fn(mi, &format!("{}::<static>", c.name));
        let sthis = self.m.funcs[si as usize].this;
        self.m.add_tok(sthis, cobj);
        self.static_init_fn.insert(tm, si);
    }

    pub(crate) fn synthetic_fn(&mut self, mi: u32, name: &str) -> FnIdx {
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx));
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: format!("{}:{}", self.mods[mi as usize].path_short(), name),
            params: vec![],
            rest: None,
            this,
            ret,
            args_all,
            token: tok,
            ctor_of: None,
            lexical_this: false,
        });
        fx
    }

    fn link_class(&mut self, mi: u32, c: &Class) {
        let tm = self.mods[mi as usize].classes[&c.name];
        let parent = c
            .extends_name
            .as_ref()
            .and_then(|n| self.resolve_class_name(mi, n));
        let builtin_parent = c
            .extends_name
            .as_deref()
            .filter(|n| parent.is_none() && crate::builtins::native_parent_summarized(n));
        if let Some(p) = parent {
            self.m.tmpls[tm as usize].parent = Some(p);
        } else if let Some(n) = builtin_parent {
            self.m.tmpls[tm as usize].native_parent = Some(("builtin".into(), n.to_string()));
        } else if c.extends_name.as_deref() == Some("Object") && c.extends_expr.is_none() {
            // `extends Object`: the ordinary chain.
        } else if c.extends_expr.is_some()
            || (c.extends_name.is_some() && c.native_extends.is_none())
        {
            // Dynamic `extends <expr>` / unresolved parent: E11-like — the
            // parent constructor is unknown code.
            let inst = self.m.tok(TokKind::Obj(tm));
            let r = self
                .m
                .reason("E11 class with a dynamic/unresolved `extends`");
            self.m.escape(inst, r);
            self.m.mark_hard(inst, Hard::E11ReflectConstruct, r);
        }
    }

    /// A class name visible in module `mi`: its own classes, then imported
    /// bindings that hold exactly one class value.
    pub(crate) fn resolve_class_name(&mut self, mi: u32, name: &str) -> Option<TmplId> {
        if let Some(&tm) = self.mods[mi as usize].classes.get(name) {
            return Some(tm);
        }
        let imp = *self.mods[mi as usize].imports.get(name)?;
        // Import bindings are filled during solving; peek through the
        // export chain statically: an export of a class name.
        self.static_class_of_node(imp)
    }

    fn static_class_of_node(&self, n: NodeId) -> Option<TmplId> {
        let toks = self.m.nodes[n as usize].pts.to_sorted();
        let mut found = None;
        for t in toks {
            if let TokKind::ClassObj(tm) = self.m.toks[t as usize] {
                if found.is_some() {
                    return None;
                }
                found = Some(tm);
            }
        }
        found
    }

    pub(crate) fn local(&mut self, mi: u32, id: u32) -> NodeId {
        if let Some(&n) = self.mods[mi as usize].locals.get(&id) {
            return n;
        }
        let n = self.m.node_named("local", || format!("local({mi},{id})"));
        self.mods[mi as usize].locals.insert(id, n);
        n
    }

    pub(crate) fn global(&mut self, mi: u32, id: u32) -> NodeId {
        if let Some(&n) = self.mods[mi as usize].globals.get(&id) {
            return n;
        }
        let n = self.m.node_named("global", || format!("global({mi},{id})"));
        self.mods[mi as usize].globals.insert(id, n);
        n
    }

    pub(crate) fn new_arr_template(&mut self, mi: u32, what: &str) -> TmplId {
        let site = self.next_site(mi);
        let tm = self.m.tmpls.len() as TmplId;
        self.m.tmpls.push(Template {
            kind: TmplKind::Arr { module: mi, site },
            display: format!("{}:[{what}#{site}]", self.mods[mi as usize].path_short()),
            birth_keys: vec![],
            definite: vec![],
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
        });
        tm
    }

    pub(crate) fn new_bobj_template(&mut self, mi: u32, what: &'static str) -> TmplId {
        let site = self.next_site(mi);
        let tm = self.m.tmpls.len() as TmplId;
        self.m.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: mi,
                site,
                what,
            },
            display: format!("{}:{what}#{site}", self.mods[mi as usize].path_short()),
            birth_keys: vec![],
            definite: vec![],
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
        });
        tm
    }

    pub(crate) fn next_site(&mut self, mi: u32) -> SiteId {
        let s = self.mods[mi as usize].site_counter;
        self.mods[mi as usize].site_counter += 1;
        s
    }

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
            for e in &module.exports {
                match e {
                    Export::Named { local, exported } => {
                        let a = self.m.atom(exported);
                        let en = self.export_node(mi, a);
                        let src = self.scope_value(mi, local);
                        self.m.edge(src, en);
                        let f = self.m.field(ns, a);
                        self.m.edge(en, f);
                    }
                    Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        let a = self.m.atom(exported);
                        let en = self.export_node(mi, a);
                        match self.resolve_source(module, source) {
                            Some(t) => {
                                let b = self.m.atom(imported);
                                let src = self.export_node(t, b);
                                self.m.edge(src, en);
                            }
                            None => self.m.add_top(en, |_| format!("unresolved re-export source {source}")),
                        }
                        let f = self.m.field(ns, a);
                        self.m.edge(en, f);
                    }
                    Export::ExportAll { source } => match self.resolve_source(module, source) {
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
                            self.m.add_top(fa, |_| format!("unresolved export * source {source}"));
                        }
                    },
                    Export::NamespaceReExport { source, name } => {
                        let a = self.m.atom(name);
                        let en = self.export_node(mi, a);
                        match self.resolve_source(module, source) {
                            Some(t) => {
                                let tns = self.mods[t as usize].ns_tok;
                                self.m.add_tok(en, tns);
                            }
                            None => self.m.add_top(en, |_| format!("unresolved re-export source {source}")),
                        }
                        let f = self.m.field(ns, a);
                        self.m.edge(en, f);
                    }
                }
            }
            for (name, fid) in &module.exported_functions {
                if let Some(&fx) = self.mods[mi as usize].funcs.get(fid) {
                    let a = self.m.atom(name);
                    let en = self.export_node(mi, a);
                    let t = self.m.funcs[fx as usize].token;
                    self.m.add_tok(en, t);
                    let f = self.m.field(ns, a);
                    self.m.edge(en, f);
                }
            }
            for f in &module.functions {
                if f.is_exported {
                    let a = self.m.atom(&f.name);
                    let en = self.export_node(mi, a);
                    let fx = self.mods[mi as usize].funcs[&f.id];
                    let t = self.m.funcs[fx as usize].token;
                    self.m.add_tok(en, t);
                    let fl = self.m.field(ns, a);
                    self.m.edge(en, fl);
                }
            }
            for c in &module.classes {
                if c.is_exported {
                    let a = self.m.atom(&c.name);
                    let en = self.export_node(mi, a);
                    let tm = self.mods[mi as usize].classes[&c.name];
                    let t = self.m.tok(TokKind::ClassObj(tm));
                    self.m.add_tok(en, t);
                    let fl = self.m.field(ns, a);
                    self.m.edge(en, fl);
                }
            }
            for name in &module.exported_objects {
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
        let keys: Vec<(u32, Atom)> = self.exports.keys().copied().collect();
        for (mi, a) in keys {
            let ns = self.mods[mi as usize].ns_tok;
            let en = self.exports[&(mi, a)];
            let fl = self.m.field(ns, a);
            self.m.edge(fl, en);
        }
    }

    fn resolve_source(&self, module: &Module, source: &str) -> Option<u32> {
        let imp = module.imports.iter().find(|i| i.source == source)?;
        let p = imp.resolved_path.as_ref()?;
        self.path_index.get(p).copied()
    }

    /// A module-scope name's value node.
    pub(crate) fn scope_value(&mut self, mi: u32, name: &str) -> NodeId {
        if let Some(&n) = self.mods[mi as usize].scope.get(name) {
            return n;
        }
        if let Some(&n) = self.mods[mi as usize].imports.get(name) {
            return n;
        }
        self.m.top_val(&format!("unresolved scope name {name}"))
    }

    // ----------------------------------------------------------- walking

    pub fn walk_all(&mut self) {
        let modules = self.modules.clone();
        // Constructor phase analysis first (needs every class).
        for (mi, (_, module)) in modules.iter().enumerate() {
            for c in &module.classes {
                let tm = self.mods[mi].classes[&c.name];
                let ph = crate::ctor_phase::analyze(c);
                self.ctor_phase.insert(tm, ph);
            }
        }
        // Parent leaks close the child's phase at `super()`.
        let tms: Vec<TmplId> = self.ctor_phase.keys().copied().collect();
        for tm in tms.iter().copied() {
            let mut leaks = false;
            let mut cur = self.m.tmpls[tm as usize].parent;
            while let Some(p) = cur {
                if self.ctor_phase.get(&p).is_some_and(|ph| ph.leaks) {
                    leaks = true;
                }
                cur = self.m.tmpls[p as usize].parent;
            }
            if self.m.tmpls[tm as usize].native_parent.is_some() {
                leaks = true;
            }
            if let Some(ph) = self.ctor_phase.get_mut(&tm) {
                ph.parent_leaks = leaks;
            }
        }
        // Birth keys: root → leaf.
        for (mi, (_, module)) in modules.iter().enumerate() {
            for c in &module.classes {
                let tm = self.mods[mi].classes[&c.name];
                self.compute_birth_keys(tm, module);
            }
        }
        for (mi, (_, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            let init_fn = self.synthetic_fn(mi, "<init>");
            let this = self.m.funcs[init_fn as usize].this;
            // Module top-level `this`: CJS `module.exports`-like object or
            // undefined in ESM; ⊤-free: a fresh literal-ish object.
            self.m.add_prim(this, prim::UNDEF);
            let fx = Fx {
                module: mi,
                f: init_fn,
                this,
                ret: self.m.funcs[init_fn as usize].ret,
                class: None,
                ctor_of: None,
                new_target: self.m.all_prim_node,
                fn_ctor: false,
            };
            for g in &module.globals {
                if let Some(init) = &g.init {
                    let v = self.expr(&fx, init);
                    let gn = self.global(mi, g.id);
                    self.m.edge(v, gn);
                }
            }
            self.stmts(&fx, &module.init);
            for f in &module.functions {
                let fxi = self.mods[mi as usize].funcs[&f.id];
                self.walk_fn(mi, fxi, f, None, None);
            }
            for c in &module.classes {
                self.walk_class(mi, c, &fx);
            }
        }
    }

    fn compute_birth_keys(&mut self, tm: TmplId, module: &Module) {
        if !self.m.tmpls[tm as usize].birth_keys.is_empty() {
            return;
        }
        let mut keys = Vec::new();
        let mut definite = Vec::new();
        if let Some(p) = self.m.tmpls[tm as usize].parent {
            if let TmplKind::Class { module: pm, .. } = self.m.tmpls[p as usize].kind {
                let pmod = self.modules[pm as usize].1;
                self.compute_birth_keys(p, pmod);
            }
            keys = self.m.tmpls[p as usize].birth_keys.clone();
            definite = self.m.tmpls[p as usize].definite.clone();
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
        for f in &c.fields {
            if f.is_private || f.key_expr.is_some() {
                continue;
            }
            let a = self.m.atom(&f.name);
            push_unique(&mut keys, a);
            push_unique(&mut definite, a);
        }
        if let Some(ph) = self.ctor_phase.get(&tm).cloned() {
            for k in &ph.assigned_order {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
            for k in &ph.definite {
                let a = self.m.atom(k);
                push_unique(&mut definite, a);
            }
        }
        self.m.tmpls[tm as usize].birth_keys = keys;
        self.m.tmpls[tm as usize].definite = definite;
    }

    pub(crate) fn walk_fn(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let info = self.m.funcs[fxi as usize].clone();
        let fn_ctor = class.is_none() && ctor_of.is_none();
        if fn_ctor {
            self.note_fn_phase(fxi, crate::ctor_phase::analyze_fn(f));
        }
        let fx = Fx {
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
            self.m.add_top(info.ret, |_| "async/generator function result (untransformed)".into());
        }
        self.stmts(&fx, &f.body);
    }

    pub(crate) fn params(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        for p in params {
            let n = self.local(fx.module, p.id);
            if let Some(d) = &p.default {
                let v = self.expr(fx, d);
                self.m.edge(v, n);
            }
        }
    }

    fn walk_class(&mut self, mi: u32, c: &Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Instance field initializers run in the field-init context.
        let fi = self.field_init_fn[&tm];
        let fthis = self.m.funcs[fi as usize].this;
        let ffx = Fx {
            module: mi,
            f: fi,
            this: fthis,
            ret: self.m.funcs[fi as usize].ret,
            class: Some(tm),
            ctor_of: Some(tm),
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        let inst = self.m.tok(TokKind::Obj(tm));
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
                None if f.key_expr.is_none() && ctor_definite.contains(&f.name) => continue,
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
                    if f.is_private {
                        // `#x`: a private slot, never reachable by name.
                        continue;
                    }
                    let a = self.m.atom(&f.name);
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
        // Static fields and static blocks: `this` is the class object.
        let si = self.static_init_fn[&tm];
        let sfx = Fx {
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
            let v = match &f.init {
                Some(e) => self.expr(&sfx, e),
                None => self.prim_node(prim::UNDEF),
            };
            let a = self.m.atom(&f.name);
            let fl = match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&sfx, k);
                    self.m.field_any(cobj)
                }
                None => self.m.field(cobj, a),
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

    fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                let n = self.local(fx.module, *id);
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
                let _ = self.expr(fx, condition);
                self.stmts(fx, then_branch);
                if let Some(e) = else_branch {
                    self.stmts(fx, e);
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
                self.stmts(fx, body);
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

impl ModInfo {
    pub fn path_short(&self) -> String {
        short_path(&self.path)
    }
}

pub fn short_path(p: &str) -> String {
    // node_modules/<pkg>/... keeps the package; otherwise the file name.
    if let Some(i) = p.rfind("node_modules/") {
        return p[i + "node_modules/".len()..].to_string();
    }
    match p.rfind('/') {
        Some(i) => p[i + 1..].to_string(),
        None => p.to_string(),
    }
}

pub(crate) fn push_unique(v: &mut Vec<Atom>, a: Atom) {
    if !v.contains(&a) {
        v.push(a);
    }
}
