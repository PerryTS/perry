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

/// The candidates a parameter's TS type picks for its entry guard: one
/// class, or the object type's keys (resolved after the census).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParamGuard {
    Class(TmplId),
    Keys(Vec<String>),
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
    /// Registry summary: only dynamic lookups read every publication.
    pub cjs_registry: NodeId,
    /// Canonical module path -> the publications for that path.
    pub cjs_paths: FxHashMap<String, NodeId>,
    /// Nonliteral publications may replace any named path.
    pub cjs_unknown_path: NodeId,
    pub cjs_main: Option<NodeId>,
    pub read_sites: Vec<ReadSite>,
    /// Exports whose value is a class declaration (design §11.2).
    pub export_class: FxHashMap<(u32, Atom), TmplId>,
    /// Exports whose value is a function declaration, and the published
    /// ones with a contract.
    pub export_fn: FxHashMap<(u32, Atom), FnIdx>,
    pub fn_contracts: Vec<((u32, Atom), FnIdx)>,
    /// Contract results accepted so far (their fields were sunk once).
    pub ret_ok: crate::pts::FxHashSet<Tok>,
    pub ret_top: crate::pts::FxHashSet<FnIdx>,
    /// Stand-ins of other units' functions (by name): value node, token.
    pub stand_in_fns: FxHashMap<String, (NodeId, Tok)>,
    /// Parameters with a TS type that picks entry-guard candidates
    /// (design §11.4), by parameter node.
    pub param_guard: FxHashMap<NodeId, ParamGuard>,
    param_ty: Vec<(NodeId, u32, perry_hir::types::Type)>,
    /// Published class exports: (export, export node, class), checked after
    /// the solve (`Gen::contract_check`).
    pub class_contracts: Vec<((u32, Atom), NodeId, TmplId)>,
    /// Per-unit run: what the unit publishes and the contracts it imports.
    pub unit_in: Option<&'a crate::units::Published>,
    /// Stand-ins of other units' classes (by template display), with the
    /// tokens whose use beyond the contract is a census report.
    pub stand_ins: FxHashMap<String, (NodeId, TmplId, Vec<Tok>)>,
    pub ctor_phase: FxHashMap<TmplId, CtorPhase>,
    pub fn_inst_of: FxHashMap<FnIdx, TmplId>,
    pub field_init_fn: FxHashMap<TmplId, FnIdx>,
    pub static_init_fn: FxHashMap<TmplId, FnIdx>,
    pub prim_const: [NodeId; 7],
    pub walked: crate::pts::FxHashSet<FnIdx>,
    pub class_envs: FxHashMap<String, NodeId>,
    /// The driver's resolution of each module's `export … from "src"`
    /// sources (module path → source → module path). A re-export declares
    /// no import of its source, so the module's imports cannot resolve it.
    pub export_sources: crate::ExportSources,
    /// Object-literal IIFE parameter node → the literal template it births.
    pub iife_birth: FxHashMap<NodeId, TmplId>,
    /// Construction-phase analysis of plain functions used as constructors.
    pub fn_phase: FxHashMap<FnIdx, CtorPhase>,
    /// Locals holding a class method's lexical home (`ClassMethodHome`):
    /// (module, local) → the class. `super` through such a home starts at
    /// that class's parent, exactly like `SuperPropertyGet`.
    pub class_homes: FxHashMap<(u32, u32), TmplId>,
    /// The unpublished object of each enclosing chunked literal birth
    /// (`Expr::LiteralChunks`), innermost last: what its chunk calls'
    /// `Expr::LiteralChunkObject` arguments pass.
    pub(crate) chunk_objects: Vec<NodeId>,
    /// Literal birth templates by expression (compiler data, `new_lit`).
    pub(crate) lit_births: FxHashMap<usize, (TmplId, Tok)>,
    /// The constant first argument of the call `call_expr` is lowering,
    /// taken by `call_with` (`Model::call_lit`).
    pub(crate) call_lit_arg: Option<Atom>,
    /// `(module, local)` of a `let require = <closure>` binding, the
    /// wrapper's synthetic require (`Gen::require_helper_body`).
    pub(crate) require_helpers: FxHashMap<(u32, u32), &'a [Stmt]>,
    pub(crate) require_helpers_indexed: std::collections::HashSet<u32>,
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
        m.cur_fn = NO_FN;
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
        let cjs_unknown_path = m.node_l("CJSUNKNOWNPATH");
        m.cjs_union = cjs_registry;
        m.cjs_unknown = cjs_unknown_path;
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
            cjs_paths: Default::default(),
            cjs_unknown_path,
            cjs_main: None,
            read_sites: Vec::new(),
            export_class: FxHashMap::default(),
            export_fn: FxHashMap::default(),
            fn_contracts: Vec::new(),
            ret_ok: Default::default(),
            ret_top: Default::default(),
            stand_in_fns: FxHashMap::default(),
            param_guard: FxHashMap::default(),
            param_ty: Vec::new(),
            class_contracts: Vec::new(),
            unit_in: None,
            stand_ins: FxHashMap::default(),
            ctor_phase: FxHashMap::default(),
            fn_inst_of: FxHashMap::default(),
            field_init_fn: FxHashMap::default(),
            static_init_fn: FxHashMap::default(),
            prim_const,
            walked: Default::default(),
            class_envs: FxHashMap::default(),
            export_sources: Default::default(),
            iife_birth: FxHashMap::default(),
            fn_phase: FxHashMap::default(),
            class_homes: FxHashMap::default(),
            chunk_objects: Vec::new(),
            lit_births: FxHashMap::default(),
            call_lit_arg: None,
            require_helpers: FxHashMap::default(),
            require_helpers_indexed: std::collections::HashSet::new(),
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
                        // Another unit's class with a contract (§11.2).
                        (ImportSpecifier::Named { imported, local }, None)
                            if self.contract_for(imp, imported).is_some() =>
                        {
                            let c = self.contract_for(imp, imported).unwrap();
                            (local.clone(), self.stand_in_class(mi, c))
                        }
                        (ImportSpecifier::Default { local }, None)
                            if self.contract_for(imp, "default").is_some() =>
                        {
                            let c = self.contract_for(imp, "default").unwrap();
                            (local.clone(), self.stand_in_class(mi, c))
                        }
                        (ImportSpecifier::Named { imported, local }, None)
                            if self.fn_contract_for(imp, imported).is_some() =>
                        {
                            let c = self.fn_contract_for(imp, imported).unwrap();
                            (local.clone(), self.stand_in_fn(mi, c))
                        }
                        // Native / builtin / unresolved modules: ⊤ values.
                        (ImportSpecifier::Named { local, .. }, None)
                        | (ImportSpecifier::Default { local }, None)
                        | (ImportSpecifier::Namespace { local }, None) => {
                            let why =
                                format!("import from native/unresolved module {}", imp.source);
                            (local.clone(), self.m.top_val(&why))
                        }
                    };
                    self.mods[mi as usize].imports.insert(local, node);
                }
            }
        }
    }

    fn fn_contract_for(&self, imp: &perry_hir::Import, name: &str) -> Option<&'a crate::units::FnContract> {
        self.unit_in?.fn_contract(imp.resolved_path.as_deref()?, name)
    }

    /// Another unit's function as this unit sees it through its contract
    /// (design §11.2): an argument the callee uses beyond reading and
    /// protocol touches escapes; for one it only reads or touches, those
    /// happen here to everything reachable from it (getters and protocol
    /// methods of this unit's objects run); the result is the layouts the
    /// contract names (stand-ins) or ⊤.
    fn stand_in_fn(&mut self, mi: u32, c: &'a crate::units::FnContract) -> NodeId {
        use crate::model::{MARK_ESCAPE, MARK_TOUCH};
        if let Some(&(n, _)) = self.stand_in_fns.get(&c.name) {
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
            (0..c.params.len()).map(|i| self.m.node_named("param", || format!("param{i}({})", c.name))).collect()
        };
        self.m.own(ret);
        self.m.own(this);
        for &p in &params {
            self.m.own(p);
        }
        self.m.cur_fn = outer_fn;
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: c.name.clone(),
            params: params.clone(),
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: c.variadic,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_step: false,
            parent: NO_FN,
        });
        self.m.sink_node(args_all, r);
        self.m.sink_node(this, r);
        for (i, &p) in params.iter().enumerate() {
            let u = c.params[i];
            if u & MARK_ESCAPE != 0 {
                self.m.sink_node(p, r);
            } else if u != 0 {
                // Everything reachable from the argument.
                self.m.cons(p, Cons::Load { key: crate::model::Key::Any, dst: p });
                if u & MARK_TOUCH != 0 {
                    self.m.cons(p, Cons::Implicit { reason: r });
                }
            }
        }
        self.m.add_prim(ret, prim::ALL);
        match &c.ret {
            Some(ds) => {
                for d in ds {
                    match self.unit_in.and_then(|u| u.layout(d)) {
                        Some(cc) => {
                            self.stand_in_class(mi, cc);
                            let tm = self.stand_ins[&cc.display].1;
                            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                            self.m.add_tok(ret, inst);
                        }
                        None => self.m.add_top(ret, |_| "result of another unit's function".into()),
                    }
                }
            }
            None => self.m.add_top(ret, |_| "result of another unit's function".into()),
        }
        let n = self.m.node_named("fnval", || format!("fnval({})", c.name));
        self.m.add_tok(n, tok);
        self.stand_in_fns.insert(c.name.clone(), (n, tok));
        n
    }

    fn contract_for(&self, imp: &perry_hir::Import, name: &str) -> Option<&'a crate::units::ClassContract> {
        self.unit_in?.class_contract(imp.resolved_path.as_deref()?, name)
    }

    /// Another unit's class as this unit sees it through its contract
    /// (design §11.2): `new` gives an instance of its template, whose facts
    /// are the defining unit's; its methods and constructor are stand-ins
    /// that hand the arguments to that unit's code and return unknown
    /// values; its fields and statics hold unknown values. Using it beyond
    /// that is this unit's census report (`Gen::reports`).
    fn stand_in_class(&mut self, mi: u32, c: &crate::units::ClassContract) -> NodeId {
        if let Some((n, _, _)) = self.stand_ins.get(&c.display) {
            return *n;
        }
        let r = self.m.reason("another unit's class used beyond its contract");
        let tm = self.m.tmpls.len() as TmplId;
        let ctor = self.stub_fn(mi, &format!("{}.constructor", c.name), tm, false, r);
        self.m.tmpls.push(Template {
            kind: TmplKind::Class {
                module: mi,
                name: c.name.clone(),
                class_id: c.class_id,
                literal_shape: c.literal_shape,
            },
            display: c.display.clone(),
            birth_keys: Vec::new(),
            definite: Vec::new(),
            parent: None,
            ctor: Some(ctor),
            native_parent: c.native_parent.clone(),
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
        });
        self.m.funcs[ctor as usize].ctor_of = Some(tm);
        self.m.foreign.insert(tm, c.facts.clone());
        let proto = self.m.tok(TokKind::Proto(tm));
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let pa = self.m.atom("prototype");
        let pf = self.m.field(cobj, pa);
        self.m.add_tok(pf, proto);
        let ca = self.m.atom("constructor");
        let cf = self.m.field(proto, ca);
        self.m.add_tok(cf, cobj);
        let any = self.m.field_any(cobj);
        self.m.add_top(any, |_| "static member of another unit's class".into());
        self.m.add_prim(any, prim::ALL);
        let mut toks = vec![inst, cobj, proto, self.m.funcs[ctor as usize].token];
        for (key, fname) in &c.methods {
            let f = self.stub_fn(mi, fname, tm, true, r);
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
            toks.push(t);
        }
        for fld in &c.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            self.m.add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        if !c.facts.fixed_layout {
            let n = self.m.field_any(inst);
            self.m.add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        let n = self.m.node_named("classval", || format!("classval({})", c.name));
        self.m.add_tok(n, cobj);
        self.stand_ins.insert(c.display.clone(), (n, tm, toks));
        n
    }

    /// A function of another unit seen through a contract: its arguments go
    /// to that unit's code, `this` admits only `class`'s instances (any other
    /// receiver escapes), the result is unknown (`ret_top`).
    fn stub_fn(&mut self, mi: u32, name: &str, class: TmplId, ret_top: bool, reason: u32) -> FnIdx {
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        let outer_fn = std::mem::replace(&mut self.m.cur_fn, fx);
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        self.m.own(ret);
        self.m.own(this);
        self.m.cur_fn = outer_fn;
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: name.to_string(),
            params: Vec::new(),
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: true,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_step: false,
            parent: NO_FN,
        });
        self.m.sink_node(args_all, reason);
        if ret_top {
            self.m.add_top(ret, |_| "result of another unit's method".into());
            self.m.add_prim(ret, prim::ALL);
        }
        self.m.cons(this, Cons::ContractThis { tmpl: class, reason });
        fx
    }

    /// After the solve: the other units' classes this unit used beyond their
    /// contract (census reports, design §11.3), by template display. An
    /// aborted unit reports every contract it consumed.
    pub(crate) fn reports(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .stand_ins
            .iter()
            .filter(|(_, (_, tm, toks))| {
                self.m.aborted
                    || self.m.foreign_written.contains(tm)
                    || self.m.adds_keys.get(tm).is_some_and(|&k| k > 0)
                    || toks.iter().any(|&t| self.m.is_escaped(t) || self.m.hard.contains_key(&t))
            })
            .map(|(d, _)| d.clone())
            .collect();
        for (name, &(_, t)) in &self.stand_in_fns {
            if self.m.aborted || self.m.is_escaped(t) || self.m.hard.contains_key(&t) {
                out.push(format!("fn:{name}"));
            }
        }
        out.sort();
        out
    }

    /// Resolve the typed parameters' entry-guard candidates (every class and
    /// import is known by now).
    pub(crate) fn resolve_param_guards(&mut self) {
        for (n, mi, ty) in std::mem::take(&mut self.param_ty) {
            if let Some(gd) = self.guard_of(mi, &ty) {
                self.param_guard.insert(n, gd);
            }
        }
    }

    /// The candidates parameter type `ty` of module `mi` picks for an entry
    /// guard (design §11.4): a class (this unit's or another's through a
    /// contract), or the keys of an object type / interface / alias.
    pub(crate) fn guard_of(&self, mi: u32, ty: &perry_hir::types::Type) -> Option<ParamGuard> {
        use perry_hir::types::Type;
        match ty {
            Type::Named(n) => {
                if let Some(&tm) = self.mods[mi as usize].classes.get(n) {
                    return Some(ParamGuard::Class(tm));
                }
                if let Some(&node) = self.mods[mi as usize].imports.get(n) {
                    return self.m.nodes[node as usize].pts.to_sorted().iter().find_map(|&t| match self.m.toks[t as usize] {
                        TokKind::ClassObj(tm) => Some(ParamGuard::Class(tm)),
                        _ => None,
                    });
                }
                let module = self.modules[mi as usize].1;
                if let Some(i) = module.interfaces.iter().find(|i| &i.name == n) {
                    if !i.extends.is_empty() || !i.type_params.is_empty() {
                        return None;
                    }
                    let keys: Vec<String> = i.properties.iter().filter(|p| !p.optional).map(|p| p.name.clone()).collect();
                    return (!keys.is_empty()).then_some(ParamGuard::Keys(keys));
                }
                let a = module.type_aliases.iter().find(|a| &a.name == n && a.type_params.is_empty())?;
                self.guard_of(mi, &a.ty)
            }
            Type::Object(o) => {
                let mut keys: Vec<String> = o.properties.iter().filter(|(_, p)| !p.optional).map(|(k, _)| k.clone()).collect();
                keys.sort();
                (!keys.is_empty()).then_some(ParamGuard::Keys(keys))
            }
            _ => None,
        }
    }

    pub(crate) fn register_fn(
        &mut self,
        mi: u32,
        id: u32,
        name: &str,
        params: &[perry_hir::Param],
        lexical_this: bool,
        is_closure: bool,
    ) -> FnIdx {
        if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
            return f;
        }
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        // Parameters and the result belong to the new function's body.
        let outer_fn = std::mem::replace(&mut self.m.cur_fn, fx);
        self.m.own(ret);
        // `this` is the function's own interface term, like a parameter.
        self.m.own(this);
        let mut ps = Vec::new();
        let mut rest = None;
        let mut mapped = None;
        for p in params {
            let n = self.local(mi, p.id);
            if p.is_rest {
                // The rest local holds a fresh array whose elements are the
                // extra arguments (and, for `arguments`, every argument).
                let arr = self.new_arr_template(mi, "rest");
                let at = self.m.tok(TokKind::Obj(arr, NO_CTX));
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
                if matches!(p.ty, perry_hir::types::Type::Named(_) | perry_hir::types::Type::Object(_)) {
                    self.param_ty.push((n, mi, p.ty.clone()));
                }
                ps.push(n);
            }
        }
        self.m.cur_fn = outer_fn;
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
            has_arguments: params.iter().any(|p| p.arguments_object.is_some()),
            token: tok,
            ctor_of: None,
            lexical_this,
            no_summary: false,
            gen_step: false,
            parent: if is_closure { outer_fn } else { NO_FN },
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
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
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
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
            let a = self.m.atom(name);
            self.m.accessor_atoms.insert(a);
            let ft = self.m.funcs[f as usize].token;
            self.m.accessors.entry((proto, a)).or_default().0.push(ft);
            push_unique(self.m.accessor_keys.entry(proto).or_default(), a);
        }
        for (name, s) in &c.setters {
            let f = self.register_fn(mi, s.id, &s.name, &s.params, false, false);
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
            let a = self.m.atom(name);
            self.m.accessor_atoms.insert(a);
            let ft = self.m.funcs[f as usize].token;
            self.m.accessors.entry((proto, a)).or_default().1.push(ft);
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
                let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                self.m.add_tok(this, inst);
                self.m.funcs[f as usize].no_summary = true;
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
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
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
            has_arguments: false,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: false,
            gen_step: false,
            parent: NO_FN,
        });
        fx
    }

    fn link_class(&mut self, mi: u32, c: &Class) {
        let tm = self.mods[mi as usize].classes[&c.name];
        if let Some(origin) = c
            .specialized_from
            .as_deref()
            .and_then(|name| self.resolve_class_name(mi, name))
        {
            self.m.constructor_origins.insert(tm, origin);
            self.m.rel(|| format!("ctororigin {tm} {origin}"));
        }
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
            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
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
            if self.m.node_fn[n as usize] != self.m.cur_fn {
                if self.m.cur_fn != NO_FN {
                    let f = self.m.cur_fn;
                    self.m.captures.insert((n, f));
                }
            }
            return n;
        }
        let n = self.m.node_named("local", || format!("local({mi},{id})"));
        self.m.own(n);
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
        self.note_birth_fn(tm);
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

    /// A generation-time birth template belongs to the body making it.
    pub(crate) fn note_birth_fn(&mut self, tm: TmplId) {
        if self.m.cur_fn != NO_FN {
            let f = self.m.cur_fn;
            self.m.tmpl_fn.insert(tm, f);
        }
    }

    pub(crate) fn new_bobj_template(&mut self, mi: u32, what: &'static str) -> TmplId {
        let site = self.next_site(mi);
        let tm = self.m.tmpls.len() as TmplId;
        self.note_birth_fn(tm);
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
                        if let Some(&tm) = self.mods[mi as usize].classes.get(local) {
                            self.export_class.insert((mi, a), tm);
                        }
                        if let Some(f) = module.functions.iter().find(|f| &f.name == local) {
                            if let Some(&fx) = self.mods[mi as usize].funcs.get(&f.id) {
                                self.export_fn.insert((mi, a), fx);
                            }
                        }
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
            for (name, fid) in &module.exported_functions {
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
            for f in &module.functions {
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
            for c in &module.classes {
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
                Some(tm) if published.is_some_and(|p| !p.broken(&self.m.tmpls[tm as usize].display)) => {
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
                    }
                    self.m.sink_node(n, r)
                }
            }
        }
        for mi in 0..self.mods.len() {
            if published.is_none_or(|p| p.whole(path_of(self, mi as u32))) {
                let t = self.mods[mi].ns_tok;
                self.m.escape(t, r);
            }
        }
        let mut paths: Vec<(String, NodeId)> =
            self.cjs_paths.iter().map(|(k, &n)| (k.clone(), n)).collect();
        paths.sort_unstable();
        let foreign = |g: &Self, k: &str| !g.path_index.contains_key(k);
        for (k, n) in paths {
            if foreign(self, &k) {
                self.m.add_top(n, |_| "CommonJS module of another unit".into());
            }
            self.m.sink_node(n, r);
        }
        for n in [self.cjs_registry, self.cjs_unknown_path].into_iter().chain(self.cjs_main) {
            self.m.add_top(n, |_| "CommonJS registry shared with other units".into());
            self.m.sink_node(n, r);
        }
        let thrown = self.thrown;
        self.m.sink_node(thrown, r);
        let gt = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        self.m.escape(gt, r);
        let holder = self.m.node_l("other-unit-receiver");
        self.m.add_tok(holder, TOP);
        let src = self.m.top_tok_node;
        self.m.cons(holder, Cons::Store { key: Key::Any, src, site: 0 });
    }

    /// After a solve: a published class export holds its class value only
    /// (a module may rebind the name); otherwise it escapes after all. True
    /// when something changed (solve again).
    pub(crate) fn contract_check(&mut self) -> bool {
        let r = self.m.reason("published to another unit");
        let mut changed = false;
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
        // A contract function's result: literal-shape objects only (their
        // fields go to the caller's unit, so they leave); anything else and
        // it escapes as before.
        let rets = self.m.contract_rets.clone();
        for (f, ret) in rets {
            if self.ret_top.contains(&f) {
                continue;
            }
            let toks = self.m.nodes[ret as usize].pts.to_sorted();
            let ok = toks.iter().all(|&t| match self.m.toks[t as usize] {
                TokKind::Obj(tm, _) => matches!(self.m.tmpls[tm as usize].kind, TmplKind::Class { literal_shape: true, .. }),
                _ => false,
            });
            if !ok {
                self.ret_top.insert(f);
                self.m.sink_node(ret, r);
                changed = true;
                continue;
            }
            for t in toks {
                if self.ret_ok.insert(t) {
                    let all = self.m.all_fields(t);
                    self.m.cons(all, Cons::TopSink { reason: r });
                    let rl = self.m.node_l("contract-ret-load");
                    self.m.sink_node(rl, r);
                    self.m.accessor_walk(t, t, crate::model::Key::Any, crate::model::ChainAct::Load(rl));
                    changed = true;
                }
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
            // Over a bound: the program gets no facts; stop generating.
            if self.m.aborted {
                return;
            }
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
                if self.m.aborted {
                    return;
                }
                let fxi = self.mods[mi as usize].funcs[&f.id];
                self.walk_fn(mi, fxi, f, None, None);
            }
            for c in &module.classes {
                if self.m.aborted {
                    return;
                }
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
            self.m.add_top(info.ret, |_| {
                "async/generator function result (untransformed)".into()
            });
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

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.stmt_inner(fx, s);
        self.m.cur_fn = prev;
    }

    fn stmt_inner(&mut self, fx: &Fx, s: &Stmt) {
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
