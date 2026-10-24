//! Constraint generation, part 1: program setup (modules, classes,
//! functions, imports/exports) and statements. Expressions live in
//! `gen_expr.rs`; the exhaustive variant list in `gen_rest.rs`.

use crate::ctor_phase::CtorPhase;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::{Class, Export, Function, Global, Import, ImportSpecifier, Module, Stmt};

mod export;
mod glue;
mod walk;
pub(crate) use walk::variant_name;

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
    /// By (name, the stand-in template it is a field of, if any).
    pub stand_in_fns: FxHashMap<(String, Option<(TmplId, bool)>), (NodeId, Tok)>,
    /// DECISIONS 45: what other units' functions let escape, or touch, of
    /// what their arguments reach: one node each for the unit.
    pub reach_escape: Option<NodeId>,
    pub reach_touch: Option<NodeId>,
    /// The contracts behind the stand-ins: a stand-in template's class or
    /// layout contract, a stand-in function token's function contract.
    pub(crate) stand_in_cc: FxHashMap<TmplId, crate::units::ClassContract>,
    /// Store effects of other units performed here (decision 17): the
    /// (instance, key) pairs done, the unknown value and the writer site.
    effects_done: crate::pts::FxHashSet<(Tok, Atom)>,
    effect_src: Option<(NodeId, SiteId)>,
    pub(crate) stand_in_fc: FxHashMap<Tok, &'a crate::units::FnContract>,
    /// Classes with a parent value (§17.2), fed after the bodies.
    pub(crate) parent_values: Vec<(u32, TmplId, Option<String>)>,
    /// Classes whose evaluated parent fed the parent value.
    pub(crate) parent_fed: std::collections::BTreeSet<TmplId>,
    /// Published module-scope value bindings (export, export node), checked
    /// after the solve (`Gen::contract_check`, design §12 value contracts).
    pub value_contracts: Vec<((u32, Atom), NodeId)>,
    /// Fields of published literals that hold only functions of this unit
    /// not escaped before: those functions are contracts (`contract_check`).
    pub field_fns: FxHashMap<(Tok, Atom), Vec<FnIdx>>,
    /// Parameters with a TS type that picks entry-guard candidates
    /// (design §11.4), by parameter node.
    pub(crate) param_guard: FxHashMap<NodeId, ParamGuard>,
    param_ty: Vec<(NodeId, u32, perry_hir::types::Type)>,
    /// Published class exports: (export, export node, class), checked after
    /// the solve (`Gen::contract_check`).
    pub class_contracts: Vec<((u32, Atom), NodeId, TmplId)>,
    /// Per-unit run: what the unit publishes and the contracts it imports.
    pub(crate) unit_in: Option<&'a crate::units::Published>,
    /// A declaration unit's view of its module (design §12.6): the other
    /// units' declarations it names, bound to their imports.
    pub(crate) view: Option<&'a crate::decl_units::UnitView>,
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
    /// Array-typed `let` bindings: the HIR's shared-mutable capture
    /// rewriting turns such a binding into a one-element cell, read and
    /// written as `x[0]` (an evaluation owner is written the same way).
    pub cell_locals: crate::pts::FxHashSet<(u32, u32)>,
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
    /// The unit view of module `mi`, when this is a per-unit solve (one
    /// module, the view's indices into it). A view borrows the module: the
    /// items below are read through these, never copied per unit.
    fn unit_view(&self, mi: usize) -> Option<&'a crate::decl_units::UnitView> {
        self.view.filter(|_| self.modules.len() == 1 && mi == 0)
    }

    pub(crate) fn functions_of(&self, mi: usize) -> Vec<&'a Function> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.funcs.iter().map(|&k| &m.functions[k]).collect(),
            None => m.functions.iter().collect(),
        }
    }

    pub(crate) fn classes_of(&self, mi: usize) -> Vec<&'a Class> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.classes.iter().map(|&k| &m.classes[k]).collect(),
            None => m.classes.iter().collect(),
        }
    }

    pub(crate) fn globals_of(&self, mi: usize) -> Vec<&'a Global> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.globals.iter().map(|&k| &m.globals[k]).collect(),
            None => m.globals.iter().collect(),
        }
    }

    pub(crate) fn init_of(&self, mi: usize) -> Vec<&'a Stmt> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.init.iter().map(|&k| &m.init[k]).collect(),
            None => m.init.iter().collect(),
        }
    }

    /// The module's imports, then the unit's synthetic ones.
    pub(crate) fn imports_of(&self, mi: usize) -> Vec<&'a Import> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => m.imports.iter().chain(v.imports.iter()).collect(),
            None => m.imports.iter().collect(),
        }
    }

    pub(crate) fn exports_of(&self, mi: usize) -> &'a [Export] {
        match self.unit_view(mi) {
            Some(v) => &v.exports,
            None => &self.modules[mi].1.exports,
        }
    }

    pub(crate) fn exported_functions_of(&self, mi: usize) -> &'a [(String, u32)] {
        match self.unit_view(mi) {
            Some(v) => &v.exported_functions,
            None => &self.modules[mi].1.exported_functions,
        }
    }

    pub(crate) fn exported_objects_of(&self, mi: usize) -> &'a [String] {
        match self.unit_view(mi) {
            Some(v) => &v.exported_objects,
            None => &self.modules[mi].1.exported_objects,
        }
    }

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
            reach_escape: None,
            reach_touch: None,
            stand_in_cc: FxHashMap::default(),
            effects_done: Default::default(),
            effect_src: None,
            stand_in_fc: FxHashMap::default(),
            parent_values: Vec::new(),
            parent_fed: Default::default(),
            value_contracts: Vec::new(),
            field_fns: FxHashMap::default(),
            param_guard: FxHashMap::default(),
            param_ty: Vec::new(),
            class_contracts: Vec::new(),
            unit_in: None,
            view: None,
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
            cell_locals: Default::default(),
            lit_births: FxHashMap::default(),
            call_lit_arg: None,
            require_helpers: FxHashMap::default(),
            require_helpers_indexed: std::collections::HashSet::new(),
        }
    }

    /// A node holding exactly the primitive kinds `p`. A union of kinds is
    /// its own node: the shared all-primitives node would add `STR` and
    /// `SYMBOL`, and a string index reads every named member.
    pub fn prim_node(&mut self, p: u16) -> NodeId {
        match p {
            prim::NUM => self.prim_const[0],
            prim::STR => self.prim_const[1],
            prim::BOOL => self.prim_const[2],
            prim::NULL => self.prim_const[3],
            prim::UNDEF => self.prim_const[4],
            prim::BIGINT => self.prim_const[5],
            prim::SYMBOL => self.prim_const[6],
            prim::ALL => self.m.all_prim_node,
            _ => {
                let n = self.m.node_l("PRIMS");
                self.m.own(n);
                self.m.add_prim(n, p);
                n
            }
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
            for f in self.functions_of(mi as usize) {
                self.register_fn(mi, f.id, &f.name, &f.params, false, false);
            }
            for c in self.classes_of(mi as usize) {
                self.register_class(mi, c);
            }
            // Closures are registered when the walker meets them.
            let _ = module;
        }
        // Class parents and native parents (second pass: names resolve).
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                self.link_class(mi as u32, c);
            }
        }
        // Module scope names (top-level Lets, functions, classes).
        for mi in 0..modules.len() {
            let mi = mi as u32;
            for s in self.init_of(mi as usize) {
                if let Stmt::Let { id, name, .. } = s {
                    let n = self.local(mi, *id);
                    self.mods[mi as usize].scope.insert(name.clone(), n);
                }
            }
            for f in self.functions_of(mi as usize) {
                let fx = self.mods[mi as usize].funcs[&f.id];
                let tok = self.m.funcs[fx as usize].token;
                let n = self.m.node_named("fnval", || format!("fnval({})", f.name));
                self.m.add_tok(n, tok);
                self.mods[mi as usize]
                    .scope
                    .entry(f.name.clone())
                    .or_insert(n);
            }
            for c in self.classes_of(mi as usize) {
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
            for g in self.globals_of(mi as usize) {
                let n = self.global(mi, g.id);
                self.mods[mi as usize]
                    .scope
                    .entry(g.name.clone())
                    .or_insert(n);
            }
        }
        // Imports bind to the exporting module's export nodes.
        for mi in 0..modules.len() {
            let mi = mi as u32;
            for imp in self.imports_of(mi as usize) {
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
                        // Another unit's module-scope value with a value
                        // contract (§12 value contracts).
                        (ImportSpecifier::Named { imported, local }, None)
                            if self.value_contract_for(imp, imported).is_some() =>
                        {
                            let c = self.value_contract_for(imp, imported).unwrap();
                            (local.clone(), self.stand_in_value(mi, local, c))
                        }
                        (ImportSpecifier::Default { local }, None)
                            if self.value_contract_for(imp, "default").is_some() =>
                        {
                            let c = self.value_contract_for(imp, "default").unwrap();
                            (local.clone(), self.stand_in_value(mi, local, c))
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

    fn fn_contract_for(
        &self,
        imp: &perry_hir::Import,
        name: &str,
    ) -> Option<&'a crate::units::FnContract> {
        self.unit_in?
            .fn_contract(imp.resolved_path.as_deref()?, name)
    }

    /// Another unit's function as this unit sees it through its contract
    /// (design §11.2): an argument the callee uses beyond reading and
    /// protocol touches escapes; for one it only reads or touches, those
    /// happen here to everything reachable from it (getters and protocol
    /// methods of this unit's objects run); the result is the layouts the
    /// contract names (stand-ins) or ⊤.
    fn stand_in_fn(&mut self, mi: u32, c: &'a crate::units::FnContract) -> NodeId {
        self.stand_in_fn_of(mi, c, None)
    }

    /// `stand_in_fn` for the function a field of stand-in template `holder`
    /// holds: called as a method, its receiver is that object, which the
    /// defining unit runs it on (`note_field_fns`); any other receiver
    /// escapes. An arrow function ignores its receiver. A static member
    /// (`holder.1`) is called on the class object instead.
    fn stand_in_fn_of(
        &mut self,
        mi: u32,
        c: &'a crate::units::FnContract,
        holder: Option<(TmplId, bool)>,
    ) -> NodeId {
        use crate::model::{MARK_ESCAPE, MARK_TOUCH};
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
            forwards_super: false,
            parent: NO_FN,
        });
        // Registered before its result is built: a result layout whose
        // field holds this function reaches it again (`stand_in_class`).
        let n = self.m.node_named("fnval", || format!("fnval({})", c.name));
        self.m.add_tok(n, tok);
        self.stand_in_fns.insert((c.name.clone(), holder), (n, tok));
        self.stand_in_fc.insert(tok, c);
        self.m.sink_node(args_all, r);
        match holder {
            _ if c.lexical_this => {}
            Some((tm, statics)) => self.m.cons(
                this,
                Cons::ContractThis {
                    tmpl: tm,
                    reason: r,
                    statics,
                },
            ),
            None => self.m.sink_node(this, r),
        }
        for (i, &p) in params.iter().enumerate() {
            let u = c.params[i];
            let ru = c.reach[i];
            if u & MARK_ESCAPE == 0 {
                // Result transfers: the argument itself, or what it reaches,
                // may come back as the result.
                if c.ret_params.contains(&(i as u32)) {
                    self.m.edge(p, ret);
                }
                if c.ret_reach.contains(&(i as u32)) {
                    let q = self
                        .m
                        .node_named("reach", || format!("reach{i}({})", c.name));
                    self.m.own(q);
                    self.m.cons(
                        p,
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
                    self.m.edge(q, ret);
                }
            }
            if u & MARK_ESCAPE != 0 {
                self.m.sink_node(p, r);
                continue;
            }
            if u & MARK_TOUCH != 0 {
                self.m.cons(p, Cons::Implicit { reason: r });
            }
            // What the argument reaches, used apart from the argument
            // (DECISIONS 45). An escape is transitive (`escape` sinks every
            // field): the argument's fields go to the unit's escape node. A
            // touch is not: the unit's touch node closes over the fields of
            // what it holds. The union of the parameters' closures is the
            // closure of their union, so one node per use holds the same
            // facts as one per parameter. A read only runs nothing here.
            if ru & MARK_ESCAPE != 0 {
                let q = match self.reach_escape {
                    Some(q) => q,
                    None => {
                        let q = self.m.node_named("reach", || "reached(escape)".into());
                        self.m.sink_node(q, r);
                        self.reach_escape = Some(q);
                        q
                    }
                };
                self.m.cons(
                    p,
                    Cons::Load {
                        key: crate::model::Key::Any,
                        dst: q,
                    },
                );
            } else if ru & MARK_TOUCH != 0 {
                let q = match self.reach_touch {
                    Some(q) => q,
                    None => {
                        let q = self.m.node_named("reach", || "reached(touch)".into());
                        self.m.cons(
                            q,
                            Cons::Load {
                                key: crate::model::Key::Any,
                                dst: q,
                            },
                        );
                        self.m.cons(q, Cons::Implicit { reason: r });
                        self.reach_touch = Some(q);
                        q
                    }
                };
                self.m.cons(
                    p,
                    Cons::Load {
                        key: crate::model::Key::Any,
                        dst: q,
                    },
                );
            }
        }
        self.m.add_prim(ret, prim::ALL);
        if c.ret_unknown {
            self.m
                .add_top(ret, |_| "result of another unit's function".into());
        }
        match &c.ret {
            Some(ds) => {
                for d in ds {
                    // A broken layout is ⊤ to every reader (§11.3).
                    match self
                        .unit_in
                        .filter(|u| !u.broken(d))
                        .and_then(|u| u.layout(d))
                    {
                        Some(cc) => {
                            self.stand_in_class(mi, cc);
                            let tm = self.stand_ins[&cc.display].1;
                            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                            self.m.add_tok(ret, inst);
                        }
                        None => self
                            .m
                            .add_top(ret, |_| "result of another unit's function".into()),
                    }
                }
            }
            None => self
                .m
                .add_top(ret, |_| "result of another unit's function".into()),
        }
        n
    }

    fn value_contract_for(
        &self,
        imp: &perry_hir::Import,
        name: &str,
    ) -> Option<&'a crate::units::ValueContract> {
        self.unit_in?
            .value_contract(imp.resolved_path.as_deref()?, name)
    }

    /// Another unit's module-scope value as this unit sees it through its
    /// value contract: instances of the stand-ins of the layouts it may hold
    /// (their fields hold unknown values), the stand-ins of its contract
    /// classes and functions, ⊤ when it may hold an unknown value. A broken
    /// layout or class is ⊤ (§11.3). Using it beyond the contract is this
    /// unit's census report, as for any stand-in.
    fn stand_in_value(
        &mut self,
        mi: u32,
        local: &str,
        c: &'a crate::units::ValueContract,
    ) -> NodeId {
        let n = self.m.node_named("value", || format!("value({local})"));
        let broken = |g: &Self, d: &str| g.unit_in.is_some_and(|u| u.broken(d));
        for cc in &c.layouts {
            if broken(self, &cc.display) {
                self.m
                    .add_top(n, |_| "broken layout of another unit".into());
                continue;
            }
            self.stand_in_class(mi, cc);
            let tm = self.stand_ins[&cc.display].1;
            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
            self.m.add_tok(n, inst);
        }
        for cc in &c.classes {
            if broken(self, &cc.display) {
                self.m.add_top(n, |_| "broken class of another unit".into());
                continue;
            }
            let v = self.stand_in_class(mi, cc);
            self.m.edge(v, n);
        }
        for fc in &c.fns {
            if broken(self, &format!("fn:{}", fc.name)) {
                self.m
                    .add_top(n, |_| "broken function of another unit".into());
                continue;
            }
            let v = self.stand_in_fn(mi, fc);
            self.m.edge(v, n);
        }
        if c.unknown {
            self.m.add_top(n, |_| "value of another unit".into());
        }
        if c.prim != 0 {
            self.m.add_prim(n, c.prim);
        }
        n
    }

    fn contract_for(
        &self,
        imp: &perry_hir::Import,
        name: &str,
    ) -> Option<&'a crate::units::ClassContract> {
        self.unit_in?
            .class_contract(imp.resolved_path.as_deref()?, name)
    }

    /// Another unit's class as this unit sees it through its contract
    /// (design §11.2): `new` gives an instance of its template, whose facts
    /// are the defining unit's; its methods and constructor are stand-ins
    /// that hand the arguments to that unit's code and return unknown
    /// values; its fields and statics hold unknown values. Using it beyond
    /// that is this unit's census report (`Gen::reports`).
    fn stand_in_class(&mut self, mi: u32, c: &'a crate::units::ClassContract) -> NodeId {
        if let Some((n, _, _)) = self.stand_ins.get(&c.display) {
            return *n;
        }
        let r = self
            .m
            .reason("another unit's class used beyond its contract");
        let tm = self.m.tmpls.len() as TmplId;
        let ctor = self.stub_fn(mi, &format!("{}.constructor", c.name), tm, false, false, r);
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
            parent_value: None,
            parent_links: Vec::new(),
        });
        self.m.funcs[ctor as usize].ctor_of = Some(tm);
        self.m.foreign.insert(tm, c.facts.clone());
        self.stand_in_cc.insert(tm, c.clone());
        let proto = self.m.tok(TokKind::Proto(tm));
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        // A literal layout has no class object: its `constructor` is
        // Object's, found up the chain (decision 17: what an argument
        // reaches is not the stand-in's class).
        if !c.literal_shape {
            let pa = self.m.atom("prototype");
            let pf = self.m.field(cobj, pa);
            self.m.add_tok(pf, proto);
            let ca = self.m.atom("constructor");
            let cf = self.m.field(proto, ca);
            self.m.add_tok(cf, cobj);
        }
        let mut toks = vec![inst, cobj, proto, self.m.funcs[ctor as usize].token];
        // Registered before its members are built: a member function whose
        // result is this layout reaches it again (`stand_in_fn_of`).
        let n = self
            .m
            .node_named("classval", || format!("classval({})", c.name));
        self.m.add_tok(n, cobj);
        self.stand_ins
            .insert(c.display.clone(), (n, tm, Vec::new()));
        // Its static members: a static method with a function contract is
        // its stand-in (made below, its result may be this class), another
        // one a stub running on the class object, any other static an
        // unknown value; other keys only when the defining unit's class
        // object is open.
        let mut static_fns: Vec<(NodeId, &'a crate::units::FnContract)> = Vec::new();
        for (key, m) in &c.statics {
            let a = self.m.atom(key);
            let n = self.m.field(cobj, a);
            if let Some((_, fc)) = c.static_fns.iter().find(|(k, _)| k == key) {
                static_fns.push((n, fc));
                continue;
            }
            match m {
                Some(fname) => {
                    let f = self.stub_fn(mi, fname, tm, true, true, r);
                    let t = self.m.funcs[f as usize].token;
                    self.m.add_tok(n, t);
                    toks.push(t);
                }
                None => {
                    self.m
                        .add_top(n, |_| "static member of another unit's class".into());
                    self.m.add_prim(n, prim::ALL);
                }
            }
        }
        if c.statics_open {
            let any = self.m.field_any(cobj);
            self.m
                .add_top(any, |_| "static member of another unit's class".into());
            self.m.add_prim(any, prim::ALL);
        }
        for (key, fname) in &c.methods {
            let f = self.stub_fn(mi, fname, tm, true, false, r);
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
            toks.push(t);
        }
        for fld in &c.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            // A field holding only contract functions: their stand-ins.
            if let Some((_, fcs)) = c.field_fns.iter().find(|(k, _)| k == &fld.key) {
                for fc in fcs {
                    let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
                    self.m.edge(v, n);
                }
                continue;
            }
            self.m
                .add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        if c.open_keys {
            let n = self.m.field_any(inst);
            self.m
                .add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        if let Some(e) = self.stand_ins.get_mut(&c.display) {
            e.2.extend(toks);
        }
        for (field, fc) in static_fns {
            let v = self.stand_in_fn_of(mi, fc, Some((tm, true)));
            self.m.edge(v, field);
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, true)))].1;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.push(t);
            }
        }
        n
    }

    /// A function of another unit seen through a contract: its arguments go
    /// to that unit's code, `this` admits only `class`'s instances (any other
    /// receiver escapes), the result is unknown (`ret_top`).
    fn stub_fn(
        &mut self,
        mi: u32,
        name: &str,
        class: TmplId,
        ret_top: bool,
        statics: bool,
        reason: u32,
    ) -> FnIdx {
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
            forwards_super: false,
            parent: NO_FN,
        });
        self.m.sink_node(args_all, reason);
        if ret_top {
            self.m
                .add_top(ret, |_| "result of another unit's method".into());
            self.m.add_prim(ret, prim::ALL);
        }
        self.m.cons(
            this,
            Cons::ContractThis {
                tmpl: class,
                reason,
                statics,
            },
        );
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
                    || toks
                        .iter()
                        .any(|&t| self.m.is_escaped(t) || self.m.hard.contains_key(&t))
            })
            .map(|(d, _)| d.clone())
            .collect();
        for ((name, _), &(_, t)) in &self.stand_in_fns {
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
                    return self.m.nodes[node as usize]
                        .pts
                        .to_sorted()
                        .iter()
                        .find_map(|&t| match self.m.toks[t as usize] {
                            TokKind::ClassObj(tm) => Some(ParamGuard::Class(tm)),
                            _ => None,
                        });
                }
                let module = self.modules[mi as usize].1;
                if let Some(i) = module.interfaces.iter().find(|i| &i.name == n) {
                    if !i.extends.is_empty() || !i.type_params.is_empty() {
                        return None;
                    }
                    let keys: Vec<String> = i
                        .properties
                        .iter()
                        .filter(|p| !p.optional)
                        .map(|p| p.name.clone())
                        .collect();
                    return (!keys.is_empty()).then_some(ParamGuard::Keys(keys));
                }
                let a = module
                    .type_aliases
                    .iter()
                    .find(|a| &a.name == n && a.type_params.is_empty())?;
                self.guard_of(mi, &a.ty)
            }
            Type::Object(o) => {
                let mut keys: Vec<String> = o
                    .properties
                    .iter()
                    .filter(|(_, p)| !p.optional)
                    .map(|(k, _)| k.clone())
                    .collect();
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
            } else if rest.is_some() && params.iter().all(|q| q.arguments_object.is_none()) {
                // A parameter after a rest parameter is a capture slot the
                // HIR appends to a class constructor (`...a, cap`): the rest
                // takes every user argument, the `new` site in the model
                // passes nothing for the slot (DECISIONS 49).
                self.m.add_prim(n, prim::UNDEF);
            } else {
                if matches!(
                    p.ty,
                    perry_hir::types::Type::Named(_) | perry_hir::types::Type::Object(_)
                ) {
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
            forwards_super: false,
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
            parent_value: None,
            parent_links: Vec::new(),
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
            self.m.funcs[f as usize].forwards_super = forwards_super(ctor);
            self.m.tmpls[tm as usize].ctor = Some(f);
        }
        let proto = self.m.tok(TokKind::Proto(tm));
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        // An object literal's layout is no class: its `constructor` is
        // Object's, found up the chain; nothing reaches its class object
        // (decision 17).
        if !c.is_literal_shape() {
            let pa = self.m.atom("prototype");
            let pf = self.m.field(cobj, pa);
            self.m.add_tok(pf, proto);
            let ca = self.m.atom("constructor");
            let cf = self.m.field(proto, ca);
            self.m.add_tok(cf, cobj);
        }
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
            forwards_super: false,
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
            // The parent is the value of the `extends` expression (§17.2,
            // DECISIONS 46): the evaluated parent of the class expression
            // or the binding `extends_name` names feeds it
            // (`feed_parent_values`); the proto nodes read it.
            let pv = self
                .m
                .node_named("parentval", || format!("parentval({})", c.name));
            let pa = self.m.atom("prototype");
            let pp = self.load(pv, Key::Named(pa));
            self.m.tmpls[tm as usize].parent_value = Some((pv, pp));
            self.parent_values.push((mi, tm, c.extends_name.clone()));
        }
    }

    /// Class `tm`'s parent is the evaluated value `v` (§17.2). A class with
    /// no parent value (its `extends` resolved statically, or none) cannot
    /// take one: its instances escape and `v` is sunk, as before.
    pub(crate) fn feed_parent(&mut self, v: NodeId, tm: Option<TmplId>, why: &str) {
        match tm.and_then(|tm| Some((tm, self.m.tmpls[tm as usize].parent_value?))) {
            Some((tm, (pv, _))) => {
                self.m.edge(v, pv);
                self.parent_fed.insert(tm);
            }
            // The class is linked to its parent by name; the evaluated
            // `extends` value is that parent unless the binding changed.
            // The value is checked after the solve (`settle_parent_values`),
            // not given to unknown code (§17.4, DECISIONS 48).
            None if tm.is_some_and(|tm| self.m.tmpls[tm as usize].parent.is_some()) => {
                let tm = tm.unwrap();
                let chk = self.m.node_l("parent-check");
                self.m.edge(v, chk);
                self.m.parent_checks.push((tm, chk));
            }
            None => {
                self.to_sink(v, why);
                if let Some(tm) = tm {
                    let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                    let r = self.m.reason(why);
                    self.m.escape(inst, r);
                    self.m.mark_hard(inst, Hard::E11ReflectConstruct, r);
                }
            }
        }
    }

    /// After the bodies: a parent value no evaluated parent fed takes the
    /// binding `extends_name` names; with none, the parent is unknown code
    /// (⊤ runs the unknown constructor: E11 as before).
    pub(crate) fn feed_parent_values(&mut self) {
        for (mi, tm, name) in std::mem::take(&mut self.parent_values) {
            if self.parent_fed.contains(&tm) {
                continue;
            }
            let (pv, _) = self.m.tmpls[tm as usize].parent_value.unwrap();
            let bound = name.as_deref().and_then(|n| {
                let m = &self.mods[mi as usize];
                m.imports.get(n).or_else(|| m.scope.get(n)).copied()
            });
            match bound {
                Some(b) => self.m.edge(b, pv),
                None => {
                    let r = self
                        .m
                        .reason("E11 class with a dynamic/unresolved `extends`");
                    self.m.add_top(pv, |_| "unresolved `extends`".into());
                    let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                    self.m.escape(inst, r);
                    self.m.mark_hard(inst, Hard::E11ReflectConstruct, r);
                }
            }
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
        if let Some(name) = self.view.and_then(|v| v.foreign_locals.get(&id)) {
            return self.extern_value(mi, name);
        }
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
        if let Some(name) = self.view.and_then(|v| v.foreign_globals.get(&id)) {
            return self.extern_value(mi, name);
        }
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
            parent_value: None,
            parent_links: Vec::new(),
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
            parent_value: None,
            parent_links: Vec::new(),
        });
        tm
    }

    pub(crate) fn next_site(&mut self, mi: u32) -> SiteId {
        let s = self.mods[mi as usize].site_counter;
        self.mods[mi as usize].site_counter += 1;
        s
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

/// `constructor(...a) { super(...a) }` (with the capture slots and their
/// rebind / stash statements the HIR adds around it): the first parameter
/// is a rest parameter, used once, as the whole argument list of a
/// top-level `super(...a)` (DECISIONS 49).
pub(crate) fn forwards_super(f: &Function) -> bool {
    let Some(p) = f.params.first() else {
        return false;
    };
    if !p.is_rest
        || p.default.is_some()
        || f.params.iter().any(|q| q.arguments_object.is_some())
        || f.is_async
        || f.is_generator
    {
        return false;
    }
    let forwards = f.body.iter().filter(|s| is_forward_of(s, p.id)).count() == 1;
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in &f.body {
        perry_hir::analysis::collect_local_refs_stmt(s, &mut refs, &mut seen);
    }
    forwards && refs.iter().filter(|&&r| r == p.id).count() == 1
}

/// `super(...a)` as a statement, `a` being local `rest`.
pub(crate) fn is_forward_of(s: &Stmt, rest: perry_hir::types::LocalId) -> bool {
    use perry_hir::{CallArg, Expr};
    matches!(s, Stmt::Expr(Expr::SuperCallSpread(args))
        if matches!(args.as_slice(), [CallArg::Spread(Expr::LocalGet(id))] if *id == rest))
}
