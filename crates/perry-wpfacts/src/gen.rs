//! Constraint generation, part 1: program setup (modules, classes,
//! functions, imports/exports) and statements. Expressions live in
//! `gen_expr.rs`; the exhaustive variant list in `gen_rest.rs`.

use crate::ctor_phase::CtorPhase;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::{Class, Export, Function, Global, Import, ImportSpecifier, Module, Stmt};

mod export;
mod glue;
mod primitives;
mod shared_closures;
mod view;
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
    /// A call through a value (`f(x)`): `recv` is the callee node, `key`
    /// the atom `()`.
    pub value_call: bool,
    pub func: FnIdx,
}

/// Another unit's module namespace object as this unit holds it (§18.2,
/// DECISIONS 115-CWL3). A key holds what a named import of it binds, and
/// that value exists in this unit once the unit names the key: `ns.k` read
/// or called (`Gen::ns_member`) makes key `k`; any other use of the
/// namespace value (passed, stored, a computed key, a receiver its callee
/// uses) lets it reach code that may read every key, so it makes all of
/// them (`Gen::ns_whole`). The unit's code is all there is: a key it never
/// names under a namespace that never leaves those reads is no value of
/// this unit.
pub struct NsStandIn<'a> {
    pub node: NodeId,
    tok: Tok,
    mi: u32,
    /// The module's path, and the source text its reasons name.
    path: &'a str,
    src: &'a str,
    local: String,
    /// The keys made, each with the value node it holds.
    made: FxHashMap<String, NodeId>,
    whole: bool,
    /// Its keys that hold another unit's value are made (`Gen::ns_escape`).
    escaped: bool,
}

/// Whether unknown code holding another unit's function `c` reaches
/// anything its defining unit does not grant it already: a closed function
/// or one using its receiver is reported (`Gen::reports`), a result with
/// layouts is (`Gen::ns_escape`).
fn escape_carries(c: &crate::units::FnContract) -> bool {
    c.closed.is_some() || !c.lexical_this || c.ret.as_ref().is_some_and(|r| !r.is_empty())
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
    pub read_sites: Vec<ReadSite>,
    /// Exports whose value is a class declaration (design §11.2).
    pub export_class: FxHashMap<(u32, Atom), TmplId>,
    /// Exports whose value is a function declaration, and the published
    /// ones with a contract.
    pub export_fn: FxHashMap<(u32, Atom), FnIdx>,
    pub fn_contracts: Vec<((u32, Atom), FnIdx)>,
    /// Contract results accepted so far (their fields were sunk once).
    pub ret_ok: crate::pts::FxHashSet<Tok>,
    /// Published literals of this unit whose fields leave key by key
    /// (design §18.7): a field holding only literal layouts of this unit is
    /// described by them (`field_layouts`, per template and key; whether it
    /// may hold primitives too: `field_prims`); any other field is sunk
    /// (`sunk_fields`, its nodes `sunk_field_toks`). `all_sunk`: the
    /// published objects whose fields all leave.
    pub leaving: Vec<Tok>,
    pub field_layouts: FxHashMap<(TmplId, Atom), Vec<TmplId>>,
    pub field_prims: crate::pts::FxHashSet<(TmplId, Atom)>,
    /// A field holding other units' stand-ins (builtins, unknown values)
    /// and nothing of this unit (DECISIONS 88): its nodes over the
    /// instances that left, published as one value contract.
    pub field_values: FxHashMap<(TmplId, Atom), Vec<NodeId>>,
    /// DECISIONS 126: a field holding only this unit's typed arrays, none
    /// escaped or hard: published as the owner's typed arrays (their one
    /// element kind, the primitives beside them). `ta_reported`: the fields
    /// a reader's census reported (`ta <key> of <display>`): sunk.
    pub field_tas: FxHashMap<(TmplId, Atom), (Option<u8>, u16)>,
    pub ta_reported: crate::pts::FxHashSet<(String, String)>,
    /// DECISIONS 126: the stand-ins of other units' typed-array fields
    /// (report, token), one per class and key.
    pub stand_in_tas: Vec<(String, Tok)>,
    /// Design §17.16 (b): the cells this unit names (key, node), the cells
    /// whose values no contract describes (sunk, published unknown), and
    /// the cells it reads.
    pub cells: Vec<(String, NodeId)>,
    pub cell_bad: crate::pts::FxHashSet<NodeId>,
    /// DECISIONS 162: (function, parameter, array cell key) the function
    /// stores its argument into, and the markers in those cells that stand
    /// for such arguments (published by the flow, not in the cell).
    pub cell_flows: std::collections::BTreeSet<(FnIdx, u32, String)>,
    pub cell_flow_toks: crate::pts::FxHashSet<Tok>,
    pub cells_read: Vec<String>,
    /// DECISIONS 171: the cells seeded once their node is consumed.
    pub cells_deferred: Vec<(String, NodeId)>,
    /// The other units' layouts it looked up by display (DECISIONS 103).
    pub layouts_read: Vec<(String, u64)>,
    /// The other units' functions it took by contract name (DECISIONS 106).
    pub fns_read: Vec<(String, u64)>,
    /// DECISIONS 94: the arrays whose elements are a cell (key, token),
    /// and the stand-ins of other units' arrays by display.
    pub arr_cells: Vec<(String, Tok)>,
    pub stand_in_arrs: FxHashMap<String, Tok>,
    pub sunk_fields: crate::pts::FxHashSet<(TmplId, Atom)>,
    pub sunk_field_toks: crate::pts::FxHashSet<(Tok, Atom)>,
    pub all_sunk: crate::pts::FxHashSet<Tok>,
    pub ret_top: crate::pts::FxHashSet<FnIdx>,
    /// The arguments of calls through a parameter marker the contract
    /// cannot describe (DECISIONS 140): sunk, ⊤ to the reader.
    pub call_arg_top: crate::pts::FxHashSet<NodeId>,
    /// Stand-ins of other units' functions (by name): value node, token.
    /// By (name, the stand-in template it is a field of, if any).
    pub stand_in_fns: FxHashMap<(String, Option<(TmplId, bool)>), (NodeId, Tok)>,
    /// Stand-in namespaces of other units' modules (§18.2), by module path.
    pub stand_in_nss: FxHashMap<String, NsStandIn<'a>>,
    /// An import of another unit's export or module namespace not yet
    /// named by this unit's code, by (module, local): the import and the
    /// export name, `None` for the namespace (DECISIONS 115-CWL3,
    /// `Gen::bind_import`).
    pub(crate) unnamed_imports: FxHashMap<(u32, String), (&'a Import, Option<String>)>,
    /// DECISIONS 45: what other units' functions let escape, or touch, of
    /// what their arguments reach: one node each for the unit.
    pub reach_escape: Option<NodeId>,
    pub reach_touch: Option<NodeId>,
    /// The contracts behind the stand-ins: a stand-in template's class or
    /// layout contract, a stand-in function token's function contract.
    pub(crate) stand_in_cc: FxHashMap<TmplId, crate::units::ClassContract>,
    /// This unit's classes (not literal shapes) by display, the first
    /// declared of a display (`own_class`).
    own_classes: FxHashMap<String, TmplId>,
    /// The stand-ins of closed classes (design §18.13): their key
    /// (`ClassContract::closed`), template and constructor token.
    pub(crate) closed_ctors: Vec<((String, String), TmplId, Tok)>,
    /// Store effects of other units performed here (decision 17): the
    /// (instance, key) pairs done, the unknown value and the writer site.
    effects_done: crate::pts::FxHashSet<(Tok, Atom)>,
    effect_site: Option<SiteId>,
    effect_srcs: FxHashMap<(TmplId, Atom), NodeId>,
    /// Stored values of this unit's store effects that no contract
    /// describes: they escaped, the effect publishes an unknown value.
    pub(crate) effect_bad: crate::pts::FxHashSet<NodeId>,
    /// This unit's writes of other units' module-scope bindings
    /// (`local_write`): per binding (HIR id) the node of what it stores,
    /// and the bindings whose stored values no contract describes
    /// (`Gen::contract_check`: they escaped).
    pub(crate) binding_writes: FxHashMap<u32, NodeId>,
    pub(crate) write_top: crate::pts::FxHashSet<u32>,
    pub(crate) stand_in_fc: FxHashMap<Tok, &'a crate::units::FnContract>,
    /// Classes with a parent value (§17.2), fed after the bodies.
    pub(crate) parent_values: Vec<(u32, TmplId, Option<String>)>,
    /// Classes whose evaluated parent fed the parent value.
    pub(crate) parent_fed: std::collections::BTreeSet<TmplId>,
    /// Published module-scope value bindings (export, export node), checked
    /// after the solve (`Gen::contract_check`, design §12 value contracts).
    pub value_contracts: Vec<((u32, Atom), NodeId)>,
    /// A published class's named static members (class, key, field node),
    /// checked after the solve as value bindings are (DECISIONS 173).
    pub static_values: Vec<(TmplId, Atom, NodeId)>,
    /// Fields of published literals that hold only functions of this unit
    /// not escaped before: those functions are contracts (`contract_check`).
    pub field_fns: FxHashMap<(Tok, Atom), Vec<FnIdx>>,
    /// Parameters with a TS type that picks entry-guard candidates
    /// (design §11.4), by parameter node.
    pub(crate) param_guard: FxHashMap<NodeId, ParamGuard>,
    param_ty: Vec<(NodeId, u32, perry_hir::types::Type)>,
    /// Published class exports: (export, export node, class), checked after
    /// the solve (`Gen::contract_check`). No export: a class whose instance
    /// a contract result or value gives out (DECISIONS 116), no binding.
    pub class_contracts: Vec<(Option<(u32, Atom)>, NodeId, TmplId)>,
    /// Per-unit run: what the unit publishes and the contracts it imports.
    pub(crate) unit_in: Option<&'a crate::units::Published>,
    /// A declaration unit's view of its module (design §12.6): the other
    /// units' declarations it names, bound to their imports.
    pub(crate) view: Option<&'a crate::decl_units::UnitView<'a>>,
    /// Stand-ins of other units' classes (by template display), with the
    /// tokens whose use beyond the contract is a census report.
    pub stand_ins: FxHashMap<String, (NodeId, TmplId, Vec<Tok>)>,
    pub ctor_phase: FxHashMap<TmplId, CtorPhase>,
    pub fn_inst_of: FxHashMap<FnIdx, TmplId>,
    pub field_init_fn: FxHashMap<TmplId, FnIdx>,
    pub static_init_fn: FxHashMap<TmplId, FnIdx>,
    pub prim_const: [NodeId; 7],
    pub(crate) shared_closures: shared_closures::SharedClosures<'a>,
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
    /// DECISIONS 149b: the receiver-rooted stores (`rooted.rs`), by
    /// expression address.
    pub rooted_stores: std::collections::BTreeMap<usize, bool>,
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
        m.cjs_cache = m.node_l("CJSCACHE");
        m.cjs_ext = m.node_l("CJSEXT");
        // An ESM entry installs no main record: `require.main` is undefined.
        m.cjs_main = m.node_l("CJSMAIN");
        m.add_prim(m.cjs_main, prim::UNDEF);
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
            read_sites: Vec::new(),
            export_class: FxHashMap::default(),
            export_fn: FxHashMap::default(),
            fn_contracts: Vec::new(),
            ret_ok: Default::default(),
            leaving: Vec::new(),
            field_layouts: FxHashMap::default(),
            field_prims: Default::default(),
            field_values: FxHashMap::default(),
            field_tas: FxHashMap::default(),
            ta_reported: Default::default(),
            stand_in_tas: Vec::new(),
            cells: Vec::new(),
            cell_bad: Default::default(),
            cell_flows: Default::default(),
            cell_flow_toks: Default::default(),
            cells_read: Vec::new(),
            cells_deferred: Vec::new(),
            layouts_read: Vec::new(),
            fns_read: Vec::new(),
            arr_cells: Vec::new(),
            stand_in_arrs: FxHashMap::default(),
            sunk_fields: Default::default(),
            sunk_field_toks: Default::default(),
            all_sunk: Default::default(),
            ret_top: Default::default(),
            call_arg_top: Default::default(),
            stand_in_fns: FxHashMap::default(),
            stand_in_nss: FxHashMap::default(),
            unnamed_imports: FxHashMap::default(),
            reach_escape: None,
            reach_touch: None,
            stand_in_cc: FxHashMap::default(),
            own_classes: FxHashMap::default(),
            closed_ctors: Vec::new(),
            effects_done: Default::default(),
            effect_site: None,
            effect_srcs: Default::default(),
            effect_bad: Default::default(),
            binding_writes: FxHashMap::default(),
            write_top: Default::default(),
            stand_in_fc: FxHashMap::default(),
            parent_values: Vec::new(),
            parent_fed: Default::default(),
            value_contracts: Vec::new(),
            static_values: Vec::new(),
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
            shared_closures: Default::default(),
            class_envs: FxHashMap::default(),
            export_sources: Default::default(),
            iife_birth: FxHashMap::default(),
            fn_phase: FxHashMap::default(),
            rooted_stores: Default::default(),
            class_homes: FxHashMap::default(),
            chunk_objects: Vec::new(),
            cell_locals: Default::default(),
            lit_births: FxHashMap::default(),
            call_lit_arg: None,
            require_helpers: FxHashMap::default(),
            require_helpers_indexed: std::collections::HashSet::new(),
        }
    }

    // ------------------------------------------------------------ registry

    /// Register every module's functions, classes and scope before any body
    /// is walked, so cross-references resolve in one pass.
    pub fn register_all(&mut self) {
        self.index_emitted_closures();
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
                let fx = self.register_fn(mi, f.id, &f.name, &f.params, false, false);
                self.m.decls.insert(fx);
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
                        // Another unit's export: its contract (§11.2), or ⊤;
                        // its module as a namespace (§18.2). Each is made
                        // when the unit's code names it (115-CWL3).
                        (ImportSpecifier::Named { imported, local }, None) => {
                            let k = (mi, local.clone());
                            self.unnamed_imports
                                .insert(k, (imp, Some(imported.clone())));
                            continue;
                        }
                        (ImportSpecifier::Default { local }, None) => {
                            let k = (mi, local.clone());
                            self.unnamed_imports
                                .insert(k, (imp, Some("default".into())));
                            continue;
                        }
                        (ImportSpecifier::Namespace { local }, None)
                            if imp
                                .resolved_path
                                .as_deref()
                                .and_then(|p| self.ns_keys_for(p))
                                .is_some() =>
                        {
                            self.unnamed_imports
                                .insert((mi, local.clone()), (imp, None));
                            continue;
                        }
                        // Native / builtin / unresolved modules: ⊤ values.
                        (ImportSpecifier::Namespace { local }, None) => {
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

    /// What `import { name as local }` binds from another unit (module
    /// `path`, written `src`): the stand-in of its class, function or value
    /// contract, else ⊤ (a native, builtin or unresolved module, or an
    /// export without a contract). A name `path` re-exports is the binding
    /// it resolves to (§18.14, ECMAScript ResolveExport): the declaring
    /// module's contract, or that module's namespace.
    fn imported_value(
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

    fn ns_keys_for(&self, path: &str) -> Option<&'a crate::units::ModExports> {
        self.unit_in?.ns_keys(path)
    }

    /// Another unit's module namespace object (design §18.2): one object per
    /// imported module, its keys the module's export names, each holding
    /// what a named import of it binds; open (⊤ under any key) when an
    /// `export *` source is outside the program. A namespace's shape is its
    /// module's declarations, so nothing about it is guarded.
    fn stand_in_ns(&mut self, mi: u32, path: &'a str, src: &'a str, local: &str) -> NodeId {
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
    fn ns_path_of(&self, n: NodeId) -> Option<String> {
        self.stand_in_nss
            .iter()
            .find(|(_, e)| e.node == n)
            .map(|(p, _)| p.clone())
    }

    /// Key `k` of stand-in namespace `path`: what `import { k }` binds, held
    /// under `k`; `None` when `k` is no export (the read is `undefined`).
    fn ns_key(&mut self, path: &str, k: &str) -> Option<NodeId> {
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
    fn ns_escape(&mut self, path: &str) {
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
    fn bind_import(&mut self, mi: u32, name: &str) -> Option<NodeId> {
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
    fn at_module_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.m.cur_fn, NO_FN);
        let r = f(self);
        self.m.cur_fn = outer;
        r
    }

    fn fn_contract_for(&self, path: &str, name: &str) -> Option<&'a crate::units::FnContract> {
        self.unit_in?.fn_contract(path, name)
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

    /// DECISIONS 118: the value a contract function stores under `key`
    /// into parameter `dst`'s argument (`reach`: into what it reaches): the
    /// arguments its flow names, ⊤ and the primitives it stores (an unknown
    /// value when the contract names no flow).
    #[allow(clippy::too_many_arguments)]
    fn stored_value(
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
    fn fill_described(
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
    fn reach_into(&mut self, holder: NodeId, out: NodeId, label: &str, name: &str) {
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
    fn ret_through_key(
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
    fn stand_in_fn_of(
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
                self.m.cons(p, Cons::Implicit { reason: r });
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
                            self.m.cons(q, Cons::Implicit { reason: r });
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

    /// DECISIONS 106: one name, one contract, its defining unit's. A copy
    /// a cell or another unit's contract forwards only names the function:
    /// a copy is a snapshot of some earlier round, and copies circulating
    /// through forwarding units would sustain a stale contract. Read like a
    /// cell: the owner may solve after this unit, which is then solved
    /// again when the contract changes.
    fn owner_fn(&mut self, c: &'a crate::units::FnContract) -> &'a crate::units::FnContract {
        if !self.fns_read.iter().any(|(x, _)| *x == c.name) {
            let seen = self.unit_in.map_or_else(
                || crate::units::digest(&None::<&crate::units::FnContract>),
                |u| u.own_fn_seen(&c.name),
            );
            self.fns_read.push((c.name.clone(), seen));
        }
        self.unit_in.and_then(|u| u.own_fn(&c.name)).unwrap_or(c)
    }

    /// DECISIONS 107: a published method runs on what the units running it
    /// published (`Published::this_args`, the join over every unit; each
    /// admits only instances of the method's class chain, `admit_this`).
    /// Described as instances of layouts, with no unit over the bound, its
    /// `this` holds them in place of the class's receiver token
    /// (`Model::escape_this`); this unit's own display is its own class's
    /// instance. A changed join solves the owner again (`units::run`).
    /// DECISIONS 123: no receiver published yet (the running units solve
    /// after the owner) is no receiver, as a closed function's parameters
    /// hold nothing before its callers publish (decision 13): the join only
    /// grows from there.
    pub(crate) fn published_receivers(&mut self) {
        let Some(unit_in) = self.unit_in else {
            return;
        };
        if unit_in.this_args_top || unit_in.eval_scopes() {
            return;
        }
        let mut none = None;
        for f in 0..self.m.funcs.len() as FnIdx {
            let fi = &self.m.funcs[f as usize];
            if fi.module != 0
                || fi.func_id == u32::MAX
                || fi.lexical_this
                || fi.ctor_of.is_some()
                || unit_in.broken(&format!("fn:{}", fi.name))
            {
                continue;
            }
            let name = fi.name.clone();
            let Some(vc) = unit_in.this_args.get(&name) else {
                let n = *none.get_or_insert_with(|| self.m.node_l("this-args-none"));
                self.m.this_args.insert(f, n);
                continue;
            };
            let described = !vc.unknown
                && vc.prim == 0
                && !vc.layouts.is_empty()
                && vc.classes.is_empty()
                && vc.fns.is_empty()
                && vc.builtins.is_empty()
                && vc.arrays.is_empty()
                && vc.boxes.is_empty()
                && vc.layouts.iter().all(|c| !unit_in.broken(&c.display));
            if !described {
                continue;
            }
            let n = self
                .m
                .node_named("this-args", || format!("this-args({name})"));
            for cc in &vc.layouts {
                let own = (0..self.m.tmpls.len()).find(|&i| {
                    let t = &self.m.tmpls[i];
                    t.display == cc.display
                        && matches!(t.kind, TmplKind::Class { .. } | TmplKind::Lit { .. })
                        && !self.stand_ins.values().any(|s| s.1 as usize == i)
                        && !self.m.foreign.contains_key(&(i as TmplId))
                });
                let tm = match own {
                    Some(i) => i as TmplId,
                    None => {
                        self.stand_in_class(0, cc);
                        self.stand_ins[&cc.display].1
                    }
                };
                let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                self.m.add_tok(n, inst);
            }
            self.m.this_args.insert(f, n);
        }
    }

    /// DECISIONS 165: a published method no unit runs (`Published::method_runs`,
    /// the join over every unit) is called only by this unit's code: its
    /// parameter markers stand for no value of another unit (as a closed
    /// function's with no callers, decision 13), so what this unit passes
    /// is what its parameters hold (`Model::beside_values`). The join only
    /// grows, and a newly run method solves the owner again (`units::run`).
    /// A unit over the bound may run any; a call through an escaped method
    /// is a census report that breaks it. An accessor runs on a get or a
    /// set, not a call: not here.
    pub(crate) fn published_args(&mut self) {
        let Some(unit_in) = self.unit_in else {
            return;
        };
        if unit_in.this_args_top || unit_in.eval_scopes() {
            return;
        }
        for f in 0..self.m.funcs.len() as FnIdx {
            let fi = &self.m.funcs[f as usize];
            if fi.module != 0
                || fi.func_id == u32::MAX
                || fi.ctor_of.is_some()
                || fi.params.is_empty()
                || fi.name.contains("::get ")
                || fi.name.contains("::set ")
                || unit_in.broken(&format!("fn:{}", fi.name))
            {
                continue;
            }
            let n = fi.params.len();
            if !unit_in.method_runs.contains(&fi.name) {
                self.m.method_described.insert(f, vec![true; n]);
                continue;
            }
            // DECISIONS 148: a method some unit runs: its parameters hold
            // what the running units pass (`Published::method_args`, their
            // join), beside the marker, as a closed function's do
            // (decision 13, `closed_params`). A position some call passes
            // an undescribed value stays the bare marker; a run that
            // publishes nothing describable (`top`) leaves them all.
            let Some(ac) = unit_in.method_args.get(&fi.name).filter(|a| !a.top) else {
                continue;
            };
            let name = fi.name.clone();
            let described: Vec<bool> = (0..n)
                .map(|i| !ac.params.get(i).is_some_and(|vc| vc.unknown))
                .collect();
            if !described.contains(&true) {
                continue;
            }
            let mut nodes = Vec::with_capacity(n);
            for i in 0..n {
                // A call passing fewer arguments passes undefined: its
                // contract's kinds carry it (DECISIONS 77).
                let node = match ac.params.get(i) {
                    Some(_) if !described[i] => NO_NODE,
                    Some(vc) => self.stand_in_value(0, &format!("{name}:arg{i}"), vc),
                    None => {
                        let node = self.m.node_named("arg", || format!("arg{i}({name})"));
                        self.m.add_prim(node, prim::UNDEF);
                        node
                    }
                };
                nodes.push(node);
            }
            self.m.method_described.insert(f, described);
            self.m.method_params.insert(f, nodes);
        }
    }

    /// This unit's own class named by display: a class has one defining
    /// unit (a literal shape may have several, DECISIONS 112).
    fn own_class(&self, display: &str) -> Option<TmplId> {
        self.own_classes
            .get(display)
            .copied()
            .filter(|tm| !self.m.foreign.contains_key(tm))
    }

    fn value_contract_for(
        &self,
        path: &str,
        name: &str,
    ) -> Option<&'a crate::units::ValueContract> {
        self.unit_in?.value_contract(path, name)
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
            // An instance of this unit's class described back to it (a
            // caller passes this unit's instances to its closed
            // constructor, DECISIONS 133): its own template, whose facts
            // this solve makes, as for the class value (DECISIONS 120).
            let tm = match self.own_class(&cc.display) {
                Some(tm) => tm,
                None => {
                    self.stand_in_class(mi, cc);
                    self.stand_ins[&cc.display].1
                }
            };
            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
            self.m.add_tok(n, inst);
        }
        for cc in &c.classes {
            if broken(self, &cc.display) {
                self.m.add_top(n, |_| "broken class of another unit".into());
                continue;
            }
            let v = self.contract_class_value(mi, cc);
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
        for &ns in &c.builtins {
            let t = self.m.tok(TokKind::Builtin(ns));
            self.m.add_tok(n, t);
        }
        // DECISIONS 94: another unit's array.
        for d in &c.arrays {
            let t = self.stand_in_arr(mi, d, false);
            self.m.add_tok(n, t);
        }
        // DECISIONS 184: another unit's primitive method container.
        for d in &c.boxes {
            let t = self.stand_in_arr(mi, d, true);
            self.m.add_tok(n, t);
        }
        n
    }

    /// Another unit's unbroken layout named by display (a function
    /// result, a field). Read like a cell (DECISIONS 103): its publisher
    /// may solve after this unit, so the read is recorded and this unit
    /// is solved again when the layout changes.
    fn other_layout(&mut self, d: &str) -> Option<&'a crate::units::ClassContract> {
        if !self.layouts_read.iter().any(|(x, _)| x == d) {
            let seen = self.unit_in.map_or_else(
                || crate::units::digest(&None::<&crate::units::ClassContract>),
                |u| u.layout_seen(d),
            );
            self.layouts_read.push((d.to_string(), seen));
        }
        self.unit_in
            .filter(|u| !u.broken(d))
            .and_then(|u| u.layout(d))
    }

    /// DECISIONS 126: another unit's typed arrays in field `key` of class
    /// `display`, as this unit sees them: one typed array of their element
    /// kind. When it escapes or takes a hard mark here, the census reports
    /// `ta <key> of <display>` and the owner sinks the field.
    fn stand_in_ta(&mut self, mi: u32, display: &str, key: &str, elem: Option<u8>) -> Tok {
        let rep = format!("ta {key} of {display}");
        if let Some(&(_, t)) = self.stand_in_tas.iter().find(|(r, _)| *r == rep) {
            return t;
        }
        let (t, _) = self.bobj(mi, "TypedArray");
        if let (Some(k), Some(tm)) = (elem, self.m.tmpl_of(t)) {
            self.m.ta_elem.insert(tm, k);
        }
        self.stand_in_tas.push((rep, t));
        t
    }

    /// DECISIONS 94: another unit's array as this unit sees it: an array
    /// (its methods are the builtins) whose elements are cell
    /// `arr:<display>`, what every unit holding it writes there.
    fn stand_in_arr(&mut self, mi: u32, display: &str, boxed: bool) -> Tok {
        if let Some(&t) = self.stand_in_arrs.get(display) {
            return t;
        }
        let tm = self.m.tmpls.len() as TmplId;
        self.m.tmpls.push(Template {
            kind: if boxed {
                TmplKind::BObj {
                    module: mi,
                    site: u32::MAX,
                    what: "primbox",
                }
            } else {
                TmplKind::Arr {
                    module: mi,
                    site: u32::MAX,
                }
            },
            display: display.to_string(),
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
            declared: vec![],
        });
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        self.stand_in_arrs.insert(display.to_string(), t);
        self.arr_leaves(t);
        t
    }

    fn contract_for(&self, path: &str, name: &str) -> Option<&'a crate::units::ClassContract> {
        self.unit_in?.class_contract(path, name)
    }

    /// A second description of a layout this unit already stands in for:
    /// literals of one shape that different units publish, or one class
    /// seen through two contracts. The reader relies only on what every
    /// description says (`ClassContract::join`): the facts join, a field
    /// both describe by their functions holds the functions of both, and a
    /// field, method or static the descriptions give differently holds what
    /// either may hold (⊤ unless both name functions). The stand-in only
    /// grows, so the order the contracts arrive in does not matter.
    fn widen_stand_in(&mut self, mi: u32, tm: TmplId, c: &'a crate::units::ClassContract) {
        let old = self.stand_in_cc[&tm].clone();
        let mut joined = old.clone();
        joined.join(c);
        let r = self
            .m
            .reason("another unit's class used beyond its contract");
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let top = |g: &mut Self, n: NodeId| {
            g.m.add_top(n, |_| "field of another unit's class".into());
            g.m.add_prim(n, prim::ALL);
        };
        for fld in &joined.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            let fns = |cc: &crate::units::ClassContract| {
                cc.field_fns
                    .iter()
                    .find(|(k, _)| k == &fld.key)
                    .map(|(_, f)| f.clone())
            };
            let layouts = |cc: &crate::units::ClassContract| {
                cc.field_layouts
                    .iter()
                    .find(|(k, _, _)| k == &fld.key)
                    .cloned()
            };
            let kind = |cc: &crate::units::ClassContract| {
                cc.facts
                    .fields
                    .iter()
                    .find(|f| f.key == fld.key)
                    .map(|f| (f.kind.clone(), f.definite))
            };
            // The join keeps field functions only where every description
            // names them: the old ones are wired, the new ones are `c`'s.
            if fns(&joined).is_some() {
                if let Some((_, fcs)) = c.field_fns.iter().find(|(k, _)| k == &fld.key) {
                    for fc in fcs {
                        let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
                        self.m.edge(v, n);
                    }
                }
                continue;
            }
            if layouts(&joined).is_some() && layouts(&joined) == layouts(&old) {
                continue;
            }
            if layouts(&joined).is_none() && fns(&old).is_none() && kind(&joined) == kind(&old) {
                continue;
            }
            top(self, n);
        }
        if joined.open_keys && !old.open_keys {
            let n = self.m.field_any(inst);
            top(self, n);
        }
        let proto = self.m.tok(TokKind::Proto(tm));
        for (key, fname) in &joined.methods {
            if old.methods.iter().any(|(k, f)| k == key && f == fname) {
                continue;
            }
            let f = self.stub_fn(mi, fname, tm, true, false, c.closed.is_none() || c.forwards, r);
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
        }
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        if joined.statics != old.statics || joined.static_fns != old.static_fns {
            for (key, _) in &joined.statics {
                let a = self.m.atom(key);
                let n = self.m.field(cobj, a);
                top(self, n);
            }
        }
        if joined.statics_open && !old.statics_open {
            let any = self.m.field_any(cobj);
            top(self, any);
        }
        self.m.foreign.insert(tm, joined.facts.clone());
        self.stand_in_cc.insert(tm, joined);
    }

    /// Another unit's class as this unit sees it through its contract
    /// (design §11.2): `new` gives an instance of its template, whose facts
    /// are the defining unit's; its methods and constructor are stand-ins
    /// that hand the arguments to that unit's code and return unknown
    /// values; its fields and statics hold unknown values. Using it beyond
    /// that is this unit's census report (`Gen::reports`).
    /// The class value contract `c` names here: this unit's own class
    /// when the contract names it back (a binding this unit wrote, read
    /// through its cell: DECISIONS 120), else its stand-in. The class is
    /// one object; its own unit has its template.
    fn contract_class_value(&mut self, mi: u32, c: &'a crate::units::ClassContract) -> NodeId {
        if let Some(i) = self.own_class(&c.display) {
            let n = self
                .m
                .node_named("own-class", || format!("own-class({})", c.display));
            let t = self.m.tok(TokKind::ClassObj(i));
            self.m.add_tok(n, t);
            return n;
        }
        self.stand_in_class(mi, c)
    }

    fn stand_in_class(&mut self, mi: u32, c: &'a crate::units::ClassContract) -> NodeId {
        // DECISIONS 106: the owner's layout over a forwarded copy.
        let c = self.other_layout(&c.display).unwrap_or(c);
        if let Some(&(n, tm, _)) = self.stand_ins.get(&c.display) {
            if self.stand_in_cc.get(&tm) != Some(c) {
                self.widen_stand_in(mi, tm, c);
            }
            return n;
        }
        let r = self
            .m
            .reason("another unit's class used beyond its contract");
        let tm = self.m.tmpls.len() as TmplId;
        // A closed class (design §18.13): its constructor's parameters
        // collect what this unit's constructions pass, published under the
        // class's key. Its body reads no `arguments` and has no rest
        // parameter (`lib::closed_of`), so an argument past them reaches
        // nothing; what a parameter holds goes to the defining unit's
        // constructor through the argument contract: another unit's
        // stand-in stays inside its contract there, anything else escapes
        // (`Cons::HandOver`, DECISIONS 109).
        let ctor = self.stub_fn_of(
            mi,
            &format!("{}.constructor", c.name),
            tm,
            false,
            false,
            c.closed.is_none() || c.forwards,
            r,
            c.closed.is_none(),
        );
        if let Some((path, name, n)) = &c.closed {
            let ct = self.m.funcs[ctor as usize].token;
            let outer_fn = std::mem::replace(&mut self.m.cur_fn, ctor);
            let params: Vec<NodeId> = (0..*n)
                .map(|i| {
                    let p = self
                        .m
                        .node_named("param", || format!("param{i}({}.constructor)", c.name));
                    self.m.own(p);
                    self.m.cons(
                        p,
                        Cons::HandOver {
                            reason: r,
                            ctor: ct,
                            arg: i,
                        },
                    );
                    p
                })
                .collect();
            self.m.cur_fn = outer_fn;
            self.m.funcs[ctor as usize].params = params;
            let t = self.m.funcs[ctor as usize].token;
            self.closed_ctors
                .push(((path.clone(), name.clone()), tm, t));
        }
        let birth_keys = c.birth_keys.iter().map(|k| self.m.atom(k)).collect();
        let definite = c.definite.iter().map(|k| self.m.atom(k)).collect();
        self.m.tmpls.push(Template {
            kind: TmplKind::Class {
                module: mi,
                name: c.name.clone(),
                class_id: c.class_id,
                literal_shape: c.literal_shape,
            },
            display: c.display.clone(),
            birth_keys,
            definite,
            declared: Vec::new(),
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
        // Its parent from another unit (DECISIONS 104): that class's own
        // stand-in, so an inherited method resolves up the chain as in the
        // defining unit. Unpublished, the chain ends here as before. A
        // parent of this unit is its own class (DECISIONS 181): a method the
        // subclass does not override is this unit's own code, run on the
        // subclass's object as in the whole program, not a replay of the
        // contract this unit published for it.
        if let Some(d) = &c.parent {
            if let Some(ptm) = self.own_class(d) {
                self.m.tmpls[tm as usize].parent = Some(ptm);
            } else if let Some(pc) = self.other_layout(d) {
                self.stand_in_class(mi, pc);
                let ptm = self.stand_ins[&pc.display].1;
                self.m.tmpls[tm as usize].parent = Some(ptm);
            }
        }
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
        let mut static_values: Vec<(NodeId, &'a (String, crate::units::ValueContract))> =
            Vec::new();
        for (key, m) in &c.statics {
            let a = self.m.atom(key);
            let n = self.m.field(cobj, a);
            if let Some((_, fc)) = c.static_fns.iter().find(|(k, _)| k == key) {
                static_fns.push((n, fc));
                continue;
            }
            // A data static with a value contract (DECISIONS 173): what it
            // describes, made below.
            if let Some(sv) = c.static_values.iter().find(|(k, _)| k == key) {
                static_values.push((n, sv));
                continue;
            }
            match m {
                Some(fname) => {
                    let f = self.stub_fn(
                        mi,
                        fname,
                        tm,
                        true,
                        true,
                        c.closed.is_none() || c.forwards,
                        r,
                    );
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
        let mut method_fns: Vec<(NodeId, &'a crate::units::FnContract)> = Vec::new();
        for (key, fname) in &c.methods {
            if let Some((_, fc)) = c.method_fns.iter().find(|(k, _)| k == key) {
                let a = self.m.atom(key);
                method_fns.push((self.m.field(proto, a), fc));
                continue;
            }
            let f = self.stub_fn(
                mi,
                fname,
                tm,
                true,
                false,
                c.closed.is_none() || c.forwards,
                r,
            );
            // DECISIONS 165: a call of it runs that method.
            self.m.stub_methods.insert(f);
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
            toks.push(t);
        }
        let mut nested: Vec<(NodeId, &'a (String, Vec<String>, bool))> = Vec::new();
        let mut value_fields: Vec<(NodeId, &'a (String, crate::units::ValueContract))> = Vec::new();
        let mut fn_fields: Vec<(NodeId, &'a Vec<crate::units::FnContract>)> = Vec::new();
        let mut class_fields: Vec<(NodeId, &'a crate::derive::KindFact, bool)> = Vec::new();
        for fld in &c.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            // DECISIONS 126: a field holding only the defining unit's typed
            // arrays: one stand-in typed array per class and key, whose uses
            // here the census reports (`Gen::reports`).
            if let Some((_, elem, p)) = c.field_tas.iter().find(|(k, _, _)| k == &fld.key) {
                let t = self.stand_in_ta(mi, &c.display, &fld.key, *elem);
                self.m.add_tok(n, t);
                self.m.add_prim(n, *p);
                continue;
            }
            // A field holding only the defining unit's literal layouts:
            // instances of their stand-ins (§18.7), made below.
            if let Some(fl) = c.field_layouts.iter().find(|(k, _, _)| k == &fld.key) {
                nested.push((n, fl));
                continue;
            }
            // A field forwarding other units' stand-ins: what its value
            // contract describes (DECISIONS 88), made below.
            if let Some(fv) = c.field_values.iter().find(|(k, _)| k == &fld.key) {
                value_fields.push((n, fv));
                continue;
            }
            // A field holding only contract functions: their stand-ins,
            // made below.
            if let Some((_, fcs)) = c.field_fns.iter().find(|(k, _)| k == &fld.key) {
                fn_fields.push((n, fcs));
                continue;
            }
            // A data field that holds only primitives (DECISIONS 74): their
            // kinds, as the defining unit's solve found them with every
            // unit's store effects (DECISIONS 73) performed; a read before
            // the field exists is undefined.
            let k = &fld.kind;
            if fld.data_field
                && !k.top
                && k.objs.is_empty()
                && k.funcs == 0
                && k.builtin_objs == 0
                && k.prims != 0
            {
                let undef = if fld.definite { 0 } else { prim::UNDEF };
                self.m.add_prim(n, k.prims | undef);
                continue;
            }
            // A data field that holds only instances of classes (and
            // primitives), as the defining unit's solve found them
            // (DECISIONS 110): made below.
            if fld.data_field && !k.top && !k.objs.is_empty() && k.funcs == 0 && k.builtin_objs == 0
            {
                class_fields.push((n, k, fld.definite));
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
        // After the layout's stand-in exists: a field function whose result
        // is this layout again (a literal returning itself) finds it
        // (DECISIONS 78).
        for (fnode, fcs) in fn_fields {
            for fc in fcs {
                let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
                self.m.edge(v, fnode);
            }
        }
        // DECISIONS 110 (73 for objects): a field whose published kind names
        // only this class (a tree's or a list's links) reads as this
        // stand-in's instance. The kind is the defining unit's at the
        // fixpoint, every unit's stores performed (a store of an object is
        // `None`: ⊤ there), so the field holds exactly such instances; using
        // one beyond the contract is this unit's census report. It needs no
        // other contract than this one. Another class it names reads ⊤ as
        // before: its contract would be a read by display, which the driver
        // does not track across rounds (CW2 DECISIONS 103).
        for (fnode, k, definite) in class_fields {
            if c.literal_shape || k.objs.iter().any(|d| *d != c.display) {
                self.m
                    .add_top(fnode, |_| "field of another unit's class".into());
                self.m.add_prim(fnode, prim::ALL);
                continue;
            }
            self.m.add_tok(fnode, inst);
            let undef = if definite { 0 } else { prim::UNDEF };
            self.m.add_prim(fnode, k.prims | undef);
        }
        for (fnode, (_, ds, prims)) in nested {
            for d in ds {
                // A broken layout is ⊤ to every reader (§11.3).
                match self.other_layout(d) {
                    Some(ic) => {
                        self.stand_in_class(mi, ic);
                        let itm = self.stand_ins[&ic.display].1;
                        let it = self.m.tok(TokKind::Obj(itm, NO_CTX));
                        self.m.add_tok(fnode, it);
                    }
                    None => self
                        .m
                        .add_top(fnode, |_| "field of another unit's class".into()),
                }
            }
            if *prims {
                self.m.add_prim(fnode, prim::ALL);
            }
        }
        for (fnode, (key, vc)) in value_fields.into_iter().chain(static_values) {
            let v = self.stand_in_value(mi, key, vc);
            self.m.edge(v, fnode);
        }
        for (field, fc) in static_fns {
            let v = self.stand_in_fn_of(mi, fc, Some((tm, true)));
            self.m.edge(v, field);
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, true)))].1;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.push(t);
            }
        }
        // Methods and the constructor with a function contract: their
        // stand-ins pass arguments by their marks and admit a subclass
        // instance as receiver (DECISIONS 57).
        for (field, fc) in method_fns {
            let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
            self.m.edge(v, field);
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, false)))].1;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.push(t);
            }
        }
        if let Some(fc) = &c.ctor_fn {
            self.stand_in_fn_of(mi, fc, Some((tm, false)));
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, false)))].1;
            let f = self.m.tok_fn(t);
            self.m.funcs[f as usize].ctor_of = Some(tm);
            self.m.funcs[ctor as usize].ctor_of = None;
            self.m.tmpls[tm as usize].ctor = Some(f);
            let stub = self.m.funcs[ctor as usize].token;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.retain(|&x| x != stub);
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
        forwarded: bool,
        reason: u32,
    ) -> FnIdx {
        self.stub_fn_of(mi, name, class, ret_top, statics, forwarded, reason, true)
    }

    /// `stub_fn`; `sink_args` false for a closed class's constructor, whose
    /// parameters hand their values over instead (`stand_in_class`).
    #[allow(clippy::too_many_arguments)]
    fn stub_fn_of(
        &mut self,
        mi: u32,
        name: &str,
        class: TmplId,
        ret_top: bool,
        statics: bool,
        forwarded: bool,
        reason: u32,
        sink_args: bool,
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
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded,
        });
        if sink_args {
            self.m.sink_node(args_all, reason);
        }
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
                this: u32::MAX,
            },
        );
        fx
    }

    /// After the solve: the other units' classes this unit used beyond their
    /// contract (census reports, design §11.3), by template display. An
    /// aborted unit reports every contract it consumed, but an open contract
    /// function: an aborted unit is unknown code holding it, as for one that
    /// escapes (`open_fn`).
    ///
    /// An aborted unit is unknown code holding everything its code names,
    /// not only what its walk reached before the node bound stopped it: the
    /// imports it names (its unit imports, the module imports among its
    /// names) still unnamed are made first (their stand-ins, the layouts
    /// their contracts reach, every key of a namespace), so each is
    /// reported. A module import the unit never names is not its to hold.
    pub(crate) fn reports(&mut self) -> Vec<String> {
        if self.m.aborted {
            let view = self.view;
            let mut rest: Vec<(u32, String)> = self
                .unnamed_imports
                .iter()
                .filter(|((_, local), (imp, _))| {
                    view.is_none_or(|v| {
                        v.ext_names.contains(local)
                            || crate::decl_units::module_of(&imp.source) != imp.source
                    })
                })
                .map(|(k, _)| k.clone())
                .collect();
            rest.sort_unstable();
            for (mi, name) in rest {
                self.import_value(mi, &name);
            }
        }
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
            // An aborted unit is unknown code holding everything it names
            // (ALI cause 4). It reports what no contract grants unknown code:
            // the layouts and classes it holds (it may store into them,
            // above) and a function it may call on a receiver of its own.
            // A call with ⊤ arguments of a function whose receiver is
            // unused is granted: an open function runs for any caller
            // (`open_fn`), a closed one's argument contract is ⊤ for an
            // aborted caller (the driver's `closed_refs`).
            let escaped = match self.m.aborted {
                true => !self.receiver_unused(t),
                false => self.m.is_escaped(t) && !self.open_fn(t),
            };
            if escaped || self.m.hard.contains_key(&t) {
                out.push(format!("fn:{name}"));
            }
        }
        // A value stored under a named key of it is one the owner does not
        // see (an element store is the typed array's own data).
        for (rep, t) in &self.stand_in_tas {
            let written = self.m.field_keys.get(t).is_some_and(|ks| {
                ks.iter().any(|&a| {
                    self.m.fields.get(&(*t, a)).is_some_and(|&n| {
                        !self.m.nodes[n as usize].pts.is_empty()
                            || self.m.nodes[n as usize].prim != 0
                    })
                })
            });
            if self.m.aborted || written || self.m.is_escaped(*t) || self.m.hard.contains_key(t) {
                out.push(rep.clone());
            }
        }
        out.extend(self.override_reports());
        out.sort();
        out.dedup();
        out
    }

    /// A class of this unit with another unit's class `A` as an ancestor
    /// and the keys it defines (on its prototype, as a birth key or a key
    /// its instances hold; `*` with computed members or stores): `A`'s unit
    /// reads `this.k` on its receivers through `A`'s chain only while no
    /// unit reports `override k of <A>` (DECISIONS 59).
    fn override_reports(&self) -> Vec<String> {
        let m = &self.m;
        let disp: FxHashMap<TmplId, &str> = self
            .stand_ins
            .iter()
            .map(|(d, (_, tm, _))| (*tm, d.as_str()))
            .collect();
        if disp.is_empty() {
            return Vec::new();
        }
        let open_node = |n: Option<&NodeId>| {
            n.is_some_and(|&n| !m.nodes[n as usize].pts.is_empty() || m.nodes[n as usize].prim != 0)
        };
        // The keys `t` holds a value under (a load makes an empty field).
        let held = |t: Tok| -> Vec<Atom> {
            m.field_keys
                .get(&t)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&a| open_node(m.fields.get(&(t, a))))
                .collect()
        };
        // Own keys of each class's instances.
        let mut inst: FxHashMap<TmplId, (Vec<Atom>, bool)> = FxHashMap::default();
        for (t, k) in m.toks.iter().enumerate() {
            let TokKind::Obj(tm, _) = *k else { continue };
            if !matches!(m.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
                continue;
            }
            let t = t as Tok;
            let e = inst.entry(tm).or_default();
            e.0.extend(held(t));
            e.1 |= open_node(m.field_any.get(&t));
        }
        let mut out = Vec::new();
        for (c, tp) in m.tmpls.iter().enumerate() {
            let c = c as TmplId;
            if !matches!(tp.kind, TmplKind::Class { .. })
                || disp.contains_key(&c)
                || m.foreign.contains_key(&c)
            {
                continue;
            }
            // Its other-unit ancestors, through this unit's parents.
            let mut anc: Vec<TmplId> = Vec::new();
            let mut work: Vec<TmplId> = tp
                .parent
                .into_iter()
                .chain(tp.parent_links.iter().copied())
                .collect();
            let mut seen: Vec<TmplId> = Vec::new();
            while let Some(p) = work.pop() {
                if seen.contains(&p) {
                    continue;
                }
                seen.push(p);
                if disp.contains_key(&p) {
                    anc.push(p);
                    continue;
                }
                let pp = &m.tmpls[p as usize];
                work.extend(pp.parent);
                work.extend(pp.parent_links.iter().copied());
            }
            if anc.is_empty() {
                continue;
            }
            let proto = m.tok_index.get(&TokKind::Proto(c)).copied();
            let mut keys: Vec<Atom> = tp.proto_members.clone();
            keys.extend(tp.birth_keys.iter().copied());
            let mut open = tp.has_computed_members;
            if let Some(pt) = proto {
                keys.extend(held(pt));
                open |= open_node(m.field_any.get(&pt));
            }
            if let Some((ks, o)) = inst.get(&c) {
                keys.extend(ks.iter().copied());
                open |= *o;
            }
            keys.sort_unstable();
            keys.dedup();
            for a in anc {
                if open {
                    out.push(format!("override * of {}", disp[&a]));
                }
                for &k in &keys {
                    let name = &m.atoms[k as usize];
                    if name != "constructor" {
                        out.push(format!("override {name} of {}", disp[&a]));
                    }
                }
            }
        }
        out
    }

    /// A stand-in of another unit's function that unknown code may hold
    /// (design §18.9, DECISIONS 67): an open contract function (no
    /// argument contract, decision 19) whose receiver is unused. Its
    /// defining unit already runs it for any caller with ⊤ arguments and
    /// checks its result; an escape here sinks the result (its layouts
    /// still get their census reports), so it is no report of its own.
    pub(crate) fn open_fn(&self, t: Tok) -> bool {
        self.stand_in_fc
            .get(&t)
            .is_some_and(|c| c.closed.is_none() && c.lexical_this)
    }

    /// A stand-in of another unit's function whose contract uses no
    /// receiver.
    fn receiver_unused(&self, t: Tok) -> bool {
        self.stand_in_fc.get(&t).is_some_and(|c| c.lexical_this)
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
    /// A parameter type naming an import names it (`import_value`).
    pub(crate) fn guard_of(&mut self, mi: u32, ty: &perry_hir::types::Type) -> Option<ParamGuard> {
        use perry_hir::types::Type;
        match ty {
            Type::Named(n) => {
                if let Some(&tm) = self.mods[mi as usize].classes.get(n) {
                    return Some(ParamGuard::Class(tm));
                }
                if let Some(node) = self.import_value(mi, n) {
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
        if is_closure {
            self.m.closures.insert(fx);
        }
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
            captures: Vec::new(),
            forwards_super: false,
            parent: if is_closure { outer_fn } else { NO_FN },
            forwarded: false,
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
        if !c.is_literal_shape() {
            self.own_classes.entry(display.clone()).or_insert(tm);
        }
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
            declared: Vec::new(),
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
            // Its hidden capture parameters (DECISIONS 117): slot `i` of the
            // class environment, in the order the HIR appends them.
            let caps = ctor
                .params
                .iter()
                .rev()
                .take_while(|p| p.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX))
                .count();
            self.m.funcs[f as usize].captures =
                (0..caps).map(|i| self.class_env(mi, &c.name, i)).collect();
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
        // DECISIONS 108: a function's name is its identity: contracts are
        // published and read under it (`units::run`). A class member is
        // named by its class, as the constructor is (`R::constructor`):
        // `Set::contains` and `IdentitySet::contains` are two functions.
        for meth in &c.methods {
            let name = format!("{}::{}", c.name, meth.name);
            let f = self.register_fn(mi, meth.id, &name, &meth.params, false, false);
            let a = self.m.atom(&meth.name);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
        }
        for meth in &c.static_methods {
            let name = format!("{}::static {}", c.name, meth.name);
            let f = self.register_fn(mi, meth.id, &name, &meth.params, false, false);
            let a = self.m.atom(&meth.name);
            let n = self.m.field(cobj, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
        }
        for (name, g) in &c.getters {
            let fname = format!("{}::get {name}", c.name);
            let f = self.register_fn(mi, g.id, &fname, &g.params, false, false);
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
            let a = self.m.atom(name);
            self.m.accessor_atoms.insert(a);
            let ft = self.m.funcs[f as usize].token;
            self.m.accessors.entry((proto, a)).or_default().0.push(ft);
            push_unique(self.m.accessor_keys.entry(proto).or_default(), a);
        }
        for (name, s) in &c.setters {
            let fname = format!("{}::set {name}", c.name);
            let f = self.register_fn(mi, s.id, &fname, &s.params, false, false);
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
                &format!("{}::{}#{}", c.name, cm.function.name, cm.function.id),
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
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded: false,
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

    /// DECISIONS 95/96: per unit, a class and the evaluation of its parent
    /// may be cut apart (`RegisterClassParentDynamic` in a CommonJS factory,
    /// or a class expression whose template another unit holds). The
    /// evaluated parent `v` is then cell `parent:<module>:<class>`, which the
    /// class's unit reads (`feed_parent_values`). False: this unit holds the
    /// class and `feed_parent` takes `v`.
    pub(crate) fn parent_to_cell(
        &mut self,
        mi: u32,
        class_name: &str,
        tm: Option<TmplId>,
        v: NodeId,
    ) -> bool {
        if self.unit_in.is_none() || tm.is_some_and(|tm| !self.stand_in_cc.contains_key(&tm)) {
            return false;
        }
        let key = format!(
            "parent:{}:{class_name}",
            self.mods[mi as usize].path_short()
        );
        let n = self.m.node_named("cell", || format!("cell({key})"));
        self.m.edge(v, n);
        self.cells.push((key, n));
        true
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
                self.import_value(mi, n)
                    .or_else(|| self.mods[mi as usize].scope.get(n).copied())
            });
            let cname = match &self.m.tmpls[tm as usize].kind {
                TmplKind::Class { name, .. } => Some(name.clone()),
                _ => None,
            };
            match bound {
                Some(b) => self.m.edge(b, pv),
                // DECISIONS 95: per unit, another unit may evaluate the
                // parent (`RegisterClassParentDynamic` in the CommonJS
                // factory): what it evaluates is cell `parent:<module>:<class>`.
                None if self.unit_in.is_some() && cname.is_some() => {
                    let key = format!(
                        "parent:{}:{}",
                        self.mods[mi as usize].path_short(),
                        cname.unwrap()
                    );
                    self.seed_cell(&key, pv);
                }
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
        let imp = self.import_value(mi, name)?;
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

    /// The node a write of local `id` stores into. Another unit's
    /// module-scope binding (`UnitView::written`) is written through its
    /// write effect: what this unit stores goes into one node, which this
    /// unit's reads of the binding see and which is published as a value
    /// contract, checked as a published binding's is (`contract_check`); the
    /// binding's unit holds what every unit's contract describes
    /// (`hold_binding_writes`), and publishes the binding with it.
    pub(crate) fn local_write(&mut self, mi: u32, id: u32) -> NodeId {
        let written = self
            .view
            .is_some_and(|v| v.written.iter().any(|(i, _)| *i == id));
        if !written {
            return self.local(mi, id);
        }
        if let Some(&w) = self.binding_writes.get(&id) {
            return w;
        }
        let l = self.local(mi, id);
        let w = self.at_module_scope(|g| {
            let w = g.m.node_named("written", || format!("written({id})"));
            g.m.edge(w, l);
            w
        });
        self.binding_writes.insert(id, w);
        w
    }

    /// Other units' writes of this unit's module-scope bindings: each
    /// binding holds what their contracts describe (stand-ins of the layouts,
    /// classes and functions, ⊤, the kinds), as an import of that value
    /// would. After the walk, as the closed functions' parameters
    /// (`closed_params`), before the boundary publishes the bindings.
    pub(crate) fn hold_binding_writes(&mut self) {
        let (Some(v), Some(u)) = (self.view, self.unit_in) else {
            return;
        };
        let path = crate::decl_units::unit_path(self.modules[0].0, v.unit);
        let mut seen: crate::pts::FxHashSet<u32> = Default::default();
        let lets: Vec<(u32, &'a str)> = self
            .init_of(0)
            .iter()
            .filter_map(|s| match s {
                Stmt::Let { id, name, .. } => Some((*id, name.as_str())),
                _ => None,
            })
            .collect();
        for (id, name) in lets {
            if !seen.insert(id) {
                continue;
            }
            let Some(c) = u.binding_write(&path, name) else {
                continue;
            };
            let n = self.local(0, id);
            let w = self.at_module_scope(|g| g.stand_in_value(0, name, c));
            self.m.edge(w, n);
        }
    }

    /// The other units' bindings this unit writes (`local_write`), each by
    /// its HIR id with its key: its unit's path and its name there.
    pub(crate) fn written_keys(&self) -> Vec<(u32, (String, String))> {
        let Some(v) = self.view else {
            return Vec::new();
        };
        v.written
            .iter()
            .filter_map(|(id, local)| {
                v.imports.iter().find_map(|imp| {
                    imp.specifiers.iter().find_map(|s| match s {
                        ImportSpecifier::Named { imported, local: l } if l == local => {
                            Some((*id, (imp.source.clone(), imported.clone())))
                        }
                        _ => None,
                    })
                })
            })
            .collect()
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
            declared: Vec::new(),
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
            declared: Vec::new(),
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

    /// `#u<k>` of this unit's key in unit mode (a generation-local site
    /// ordinal is unique within its unit only), else empty.
    pub(crate) fn unit_suffix(&self) -> &'a str {
        match self.view {
            Some(v) if self.unit_in.is_some() => v.key.rfind('#').map_or("", |i| &v.key[i..]),
            _ => "",
        }
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
