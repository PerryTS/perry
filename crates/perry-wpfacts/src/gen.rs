//! Constraint generation, part 1: program setup (modules, classes,
//! functions, imports/exports) and statements. Expressions live in
//! `gen_expr.rs`; the exhaustive variant list in `gen_rest.rs`.

use crate::ctor_phase::CtorPhase;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::{Class, Export, Function, Global, Import, ImportSpecifier, Module, Stmt};

mod compare;
mod define_keys;
mod export;
pub(crate) use export::UNDESCRIBED;
mod for_in;
mod glue;
pub(crate) mod narrow;
mod native_result;
mod once;
mod own_guard;
mod own_store;
mod param_key;
pub(crate) use own_guard::Region;
mod primitives;
mod private_methods;
mod shared_closures;
mod view;
mod walk;
pub(crate) mod zero_sign;
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

/// A computed member operation over nodes already generated for the solve.
#[derive(Clone, Debug)]
pub(crate) struct ElementSite {
    pub module: u32,
    pub byte_offset: u32,
    pub op: crate::derive::ElementOp,
    pub recv: NodeId,
    pub idx: NodeId,
    pub key: Key,
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
    /// The dead-zone value of a local's cell (`Gen::dead_zone`).
    pub dead_zones: FxHashMap<u32, NodeId>,
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
    /// Per for-of loop (its value local): what its step yields
    /// (`Gen::step_yield_node`).
    pub stepyield: FxHashMap<(FnIdx, u32), NodeId>,
}

/// A body generation walks once its function is active (DECISIONS 225).
pub(crate) enum PendingWalk<'a> {
    Fn(
        u32,
        FnIdx,
        &'a perry_hir::Function,
        Option<TmplId>,
        Option<TmplId>,
    ),
    Fields(u32, &'a perry_hir::Class),
    /// A closure's body (its emitted definition) in its creation context.
    Closure(crate::gen::Fx, &'a perry_hir::Expr),
    #[cfg(test)]
    OwnedClosure(crate::gen::Fx, Box<perry_hir::Expr>),
    /// A module only `import()` loads: its body runs once an `import()`
    /// site a body that can run holds names it (or a module it loads
    /// imports it).
    Module(u32),
}

pub struct Gen<'a> {
    pub m: Model,
    pub(crate) diagnostic: Option<crate::diagnostics::Capture>,
    pub modules: Vec<(&'a str, &'a Module)>,
    pub(crate) mods: Vec<ModInfo>,
    pub path_index: FxHashMap<String, u32>,
    pub exports: FxHashMap<(u32, Atom), NodeId>,
    pub export_resolving: crate::pts::FxHashSet<(u32, Atom)>,
    pub thrown: NodeId,
    /// The realm error a failed runtime operation throws (`Model::runtime_error`,
    /// held by `thrown`), for a site that throws it into its own region.
    pub runtime_error: Tok,
    /// Registry summary: only dynamic lookups read every publication.
    pub cjs_registry: NodeId,
    /// Canonical module path -> the publications for that path.
    pub cjs_paths: FxHashMap<String, NodeId>,
    /// Nonliteral publications may replace any named path.
    pub cjs_unknown_path: NodeId,
    pub read_sites: Vec<ReadSite>,
    pub(crate) element_sites: Vec<ElementSite>,
    /// `(module, element_offset, value node)` of every `IndexGet` copy
    /// (`canonical_numbers`, P3 B1).
    pub(crate) element_values: Vec<(u32, u32, NodeId)>,
    /// The result node of every named property read codegen lowers as a
    /// property read (a `PropertyGet`, a `PropertyUpdate`'s read): the reads
    /// `rest_fields`' use record admits (I32 at rest, PR C).
    pub(crate) field_reads: Vec<NodeId>,
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
    /// This unit's literals handed to another unit's closed function
    /// (decision 13) and the literals of this unit their described fields
    /// hold: the argument contract describes their fields as a published
    /// literal's (`nested_fields`), but nothing of them leaves: the closed
    /// callee's uses come back through its contract.
    pub handed: Vec<Tok>,
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
    /// DECISIONS 208: while a value contract is built
    /// (`lib::value_contract_of_toks`), the layouts of this unit it
    /// describes: each stands in `layouts_out` until the outermost one is
    /// built.
    pub(crate) describing: std::cell::RefCell<Option<Vec<String>>>,
    pub ta_reported: crate::pts::FxHashSet<(String, String)>,
    /// DECISIONS 126: the stand-ins of other units' typed-array fields
    /// (report, token), one per class and key.
    pub stand_in_tas: Vec<(String, Tok)>,
    /// Stable source-allocation ordinals, independent of imported stand-ins.
    pub indexed_allocation_sites: FxHashMap<(u32, usize), u32>,
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
    /// The node each read cell's other-unit writes are seeded into, by the
    /// cell's node (`seed_cell_now`): what the unit read there, which its
    /// own write leaves to their writers.
    pub cell_seeds: FxHashMap<NodeId, NodeId>,
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
    /// Per set of protocols (`MARK_TOUCH*` bits) the touch runs.
    pub reach_touch: Vec<(u8, NodeId)>,
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
    /// While a for-of binding is walked (`Gen::stmts`): its value local
    /// reads the value the loop's `arrayRecordForValue` just wrote.
    pub(crate) for_binding: Option<(u32, u32, NodeId)>,
    /// Loop-test narrowings being walked (`gen/narrow.rs`).
    pub(crate) narrow: Vec<narrow::Narrow>,
    /// The function body being walked and its locals a guard may narrow
    /// (`narrow::scope_locals`).
    pub(crate) narrow_scope: Option<(FnIdx, crate::pts::FxHashSet<u32>)>,
    /// The `const` bindings the bodies being walked declare
    /// (`narrow::Consts`): a body a closure creates may narrow the ones it
    /// captures.
    pub(crate) narrow_consts: crate::pts::FxHashSet<(u32, u32)>,
    /// `(module, byte offset, argument nodes)` of each method call site
    /// (`bindings::ModuleBindings::call_arg_ranges`).
    pub(crate) call_args_at: Vec<(u32, u32, Vec<NodeId>)>,
    /// `(module, byte offset, result node)` of each method call site
    /// (`bindings::ModuleBindings::number_calls`).
    pub(crate) call_results_at: Vec<(u32, u32, NodeId)>,
    /// P2 (DESIGN-p2 §2): every call site with a source offset: its HIR
    /// identity (the callee expression's address), whether it spreads, and
    /// for a direct call its callee node (`signatures::typed_calls`).
    pub(crate) call_sites_at: Vec<crate::signatures::CallSiteRec>,
    /// One-unit String literal nodes by code (`Gen::str_literal`).
    pub(crate) str_lits: FxHashMap<i64, NodeId>,
    /// The counted walks HIR lowers a for-of over an ordinary array to
    /// (`perry_hir::lower::for_of_guard`, `Gen::stmt`): (module, array
    /// local, counter local). Their `array[k]` binding is what the
    /// iteration yields, as the IteratorRecord loop's element is.
    pub(crate) for_of_walks: crate::pts::FxHashSet<(u32, u32, u32)>,
    /// The `const` bindings walked with an initializer (module, HIR id).
    pub(crate) initialized_consts: crate::pts::FxHashSet<(u32, u32)>,
    pub(crate) write_top: crate::pts::FxHashSet<u32>,
    pub(crate) stand_in_fc: FxHashMap<Tok, &'a crate::units::FnContract>,
    /// DECISIONS 241: stores on a published function no contract describes
    /// (`contract_check`): sunk, published unknown.
    pub(crate) fn_store_top: crate::pts::FxHashSet<NodeId>,
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
    /// One node per integer literal value, holding that Number with its
    /// point interval (`Gen::num_literal`); shared like `prim_const`.
    pub num_lits: FxHashMap<i64, NodeId>,
    pub(crate) shared_closures: shared_closures::SharedClosures<'a>,
    pub walked: crate::pts::FxHashSet<FnIdx>,
    /// DECISIONS 225: the bodies not walked yet because they are not
    /// active (`Gen::walk_activated` walks each once it is).
    pub(crate) pending_walk: FxHashMap<FnIdx, Vec<PendingWalk<'a>>>,
    pub class_envs: FxHashMap<String, NodeId>,
    /// The driver's resolution of each module's `export … from "src"`
    /// sources (module path → source → module path). A re-export declares
    /// no import of its source, so the module's imports cannot resolve it.
    pub export_sources: crate::ExportSources,
    /// The negative control (`PERRY_FACTS_SABOTAGE=dynimport`): `import()`
    /// sites skip their module mapping (DECISIONS 226).
    pub dynimport_sabotage: bool,
    /// Integer intervals are generated (`FactsConfig::no_int_ranges`).
    pub int_ranges: bool,
    /// Array literals record their birth length (`solve::arr_len`;
    /// `FactsConfig::no_array_lengths`).
    pub arr_lengths: bool,
    /// The source site of the last `new Array(...)` of the builtin
    /// (`new_builtin`), which the fused fill (`call_with`) marks hole-free.
    pub new_array_site: Option<SiteId>,
    /// The push-filled births and their fill pushes (`push_fills`), by
    /// expression identity.
    pub(crate) push_fill: crate::push_fills::Fills,
    /// Guards narrow locals and one-unit Strings carry codes
    /// (`FactsConfig::no_guard_ranges`).
    pub guard_ranges: bool,
    /// Guards narrow locals by presence: a truthy or non-nullish test
    /// filters out `null`/`undefined` (`FactsConfig::no_guard_presence`).
    pub guard_presence: bool,
    /// R1: a write of a narrowed local in a branch that always leaves past
    /// the narrowing's code does not end its narrowed reads after the branch
    /// (`gen::narrow`, "Writes on a leaving path";
    /// `FactsConfig::no_leaving_writes`).
    pub leaving_writes: bool,
    /// −0 tolerance (`zero_sign`): the expression `expr` evaluates next is
    /// an operand whose parent classifies its use (a compare, an integer
    /// rule, a copy, a store, a call argument); any other is a
    /// sign-observing use (`gen/zero_sign.rs`). Taken by `expr`.
    pub zs_ok: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=leavingwrite`): every
    /// branch's writes are dropped from the reads after it, also a branch
    /// that falls through to them.
    pub leaving_write_sabotage: bool,
    /// The loops and `switch`es (`jumps`) and `try` statements (`tries`)
    /// the walk is inside: where an unlabeled `break`/`continue` lands, and
    /// whether a `throw` or a `finally` stays inside a narrowing's code.
    pub jumps: u32,
    pub tries: u32,
    /// The `import()` / `require(expr)` specifiers of the bodies walked, by
    /// module (`live_dynamic_imports`): in the activation pass, the sites a
    /// body that can run holds.
    pub dynimport_seen: crate::pts::FxHashSet<(u32, String)>,
    /// Activation: the modules only `import()` loads (`dynamic_only`), and
    /// every module's body function once they exist.
    pub dynamic_only: crate::pts::FxHashSet<u32>,
    /// `FactsConfig::roots`: modules loaded whatever imports them.
    pub roots: Vec<String>,
    pub init_fn_of: FxHashMap<u32, FnIdx>,
    /// RPG1b: per module, its once closures (`once::once_closures`).
    pub once_ids: Vec<FxHashMap<u32, bool>>,
    /// RPG1b: the walk is at a statement of a function body (`order_ok`),
    /// inside that many loops of it (`StoreSite::in_loop`).
    pub order_ok: bool,
    pub loop_depth: u32,
    /// Object-literal IIFE parameter node → the literal template it births.
    pub iife_birth: FxHashMap<NodeId, TmplId>,
    /// Construction-phase analysis of plain functions used as constructors.
    pub fn_phase: FxHashMap<FnIdx, CtorPhase>,
    /// DECISIONS 149b: the receiver-rooted stores (`rooted.rs`), by
    /// expression address.
    pub rooted_stores: std::collections::BTreeMap<usize, bool>,
    /// Locals holding a class method's lexical home (`ClassMethodHome`):
    /// (module, local) → the class. `super` through such a home starts at
    /// the home's prototypes, exactly like `SuperPropertyGet`.
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
    /// A defaulted parameter's argument node by (module, local): what the
    /// calls pass, before the default replaces `undefined` (compiler data,
    /// `params_inner`).
    pub(crate) default_args: FxHashMap<(u32, u32), NodeId>,
    /// The constant first argument of the call `call_expr` is lowering,
    /// taken by `call_with` (`Model::call_lit`).
    pub(crate) call_lit_arg: Option<Atom>,
    /// Reads guarded by an own-property test (the `IndexGet`'s address)
    /// and the invalidation node watched by `Cons::OwnGate`.
    pub(crate) own_reads: FxHashMap<usize, NodeId>,
    /// Reads after a store of the same object and key (`gen/own_store.rs`,
    /// the `IndexGet`'s address).
    pub(crate) own_store_reads: crate::pts::FxHashSet<usize>,
    /// The last call's callee (`f(o,k)`), or method receiver (`H` of
    /// `H.call(o,k)`, `o` of `o.hasOwnProperty(k)`).
    pub(crate) own_test_recv: Option<NodeId>,
    /// FVA 4.3b: (module, `__keys` local) -> the `__forin_obj` local whose
    /// `ForInKeys` it holds.
    pub(crate) for_in_keys_of: FxHashMap<(u32, u32), u32>,
    /// The for-in bodies being walked: (module, loop variable, key parameter).
    pub(crate) for_in_key: Vec<(u32, u32, NodeId)>,
    /// Constant bindings' initializers: (module, global, id) -> init
    /// (`Gen::note_define_keys`, `Gen::const_key`).
    pub(crate) const_inits: FxHashMap<(u32, bool, u32), define_keys::ConstInit>,
    /// Per module: locals written or captured anywhere (`Gen::for_in_of`).
    pub(crate) unstable: FxHashMap<u32, crate::pts::FxHashSet<u32>>,
    /// Per module: its call-only functions' parameters and calls
    /// (`Gen::key_shape_of`).
    pub(crate) param_calls: FxHashMap<u32, param_key::ParamCalls<'a>>,
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
    /// The innermost `if` branch being walked: its test node and the test
    /// that selects it. An assignment to a local there reaches the local
    /// only when the test can select the branch (`Cons::Guard`); every
    /// other effect of the branch stays unconditional.
    pub guard: Option<(NodeId, u8)>,
}

#[path = "gen/import_values.rs"]
mod import_values;

#[path = "gen/class_values.rs"]
mod class_values;
mod indexed_owner;

impl<'a> Gen<'a> {
    #[cfg(test)]
    pub(crate) fn packet_generator_effect(
        &mut self,
        f: FnIdx,
        object: NodeId,
        is_async: bool,
        publish: impl FnOnce(&mut Self),
    ) {
        let mode = self.m.packet_generator_action(f, object, is_async);
        if mode.unwrap_or(true) {
            publish(self);
        }
        if mode.is_some() {
            self.m.packet_protocol_depth -= 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn packet_class_effect(
        &mut self,
        mi: u32,
        name: &str,
        action: crate::summary::packet::ClassAction<NodeId>,
        publish: impl FnOnce(&mut Self),
    ) {
        let mode = self
            .m
            .packet_class_action(self.modules[mi as usize].0, name, action);
        if mode.unwrap_or(true) {
            publish(self);
        }
        if mode.is_some() {
            self.m.packet_protocol_depth -= 1;
        }
    }

    pub fn new(modules: Vec<(&'a str, &'a Module)>, relations: bool, explain: bool) -> Self {
        Self::new_observed(modules, relations, explain, false)
    }

    pub(crate) fn new_observed(
        modules: Vec<(&'a str, &'a Module)>,
        relations: bool,
        explain: bool,
        birth_census: bool,
    ) -> Self {
        let mut m = Model::default();
        m.cur_fn = NO_FN;
        m.explain = explain;
        m.cur_src = NO_NODE;
        m.cur_why = u32::MAX;
        m.rel_phase = b'G';
        // Capture initialization origins too, before any solver propagation.
        // This state exists only for an explicitly requested diagnostic dump.
        if birth_census {
            let mut d = crate::diagnostics::Trace::default();
            d.census = Some(crate::diagnostics::BirthCensus {
                suppress: (std::env::var("PERRY_FACTS_ONLY").ok().as_deref() == Some("1"))
                    .then(|| std::env::var("PERRY_FACTS_SUPPRESS_BIRTH").ok())
                    .flatten(),
                ..Default::default()
            });
            m.diagnostic = Some(d);
        }
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
        // What a `catch` (or a rejection handler) receives beside the values
        // the program throws (edges): the error object a failed runtime
        // operation makes (`exception::throw_core`; codegen inserts such
        // throws the model never sees, so every program can catch one). It is
        // also the region of every call made outside generation (DESIGN SEEDE
        // section 8); ⊤ reaches it only through such a region.
        let rterr = m.runtime_error();
        m.add_tok(thrown, rterr);
        m.thrown = thrown;
        let esc = m.node_l("ESCRAISE");
        let r = m.reason("thrown into unknown code");
        m.sink_node(esc, r);
        m.esc_raise = esc;
        let acc = m.node_l("ESCACCESSORS");
        let r = m.reason("accessor function of a value unknown code holds");
        m.sink_node(acc, r);
        m.esc_accessors = acc;
        m.lazy_raise = m.node_l("LAZYRAISE");
        m.escaped_classes = m.node_l("ESCCLASSES");
        let cjs_registry = m.node_l("CJSREG");
        let cjs_unknown_path = m.node_l("CJSUNKNOWNPATH");
        m.cjs_union = cjs_registry;
        m.cjs_unknown = cjs_unknown_path;
        m.cjs_cache = m.node_l("CJSCACHE");
        m.cjs_ext = m.node_l("CJSEXT");
        // An ESM entry installs no main record: `require.main` is undefined.
        m.cjs_main = m.node_l("CJSMAIN");
        m.add_prim(m.cjs_main, prim::UNDEF);
        m.process_arrays = m.process_string_arrays();
        let _ = m.tok(TokKind::ObjectProto);
        let mut path_index = FxHashMap::default();
        for (i, (p, _)) in modules.iter().enumerate() {
            path_index.insert(p.to_string(), i as u32);
        }
        Gen {
            m,
            diagnostic: None,
            modules,
            mods: Vec::new(),
            path_index,
            exports: FxHashMap::default(),
            export_resolving: Default::default(),
            thrown,
            runtime_error: rterr,
            cjs_registry,
            cjs_paths: Default::default(),
            cjs_unknown_path,
            read_sites: Vec::new(),
            element_sites: Vec::new(),
            element_values: Vec::new(),
            field_reads: Vec::new(),
            export_class: FxHashMap::default(),
            export_fn: FxHashMap::default(),
            fn_contracts: Vec::new(),
            ret_ok: Default::default(),
            leaving: Vec::new(),
            handed: Vec::new(),
            field_layouts: FxHashMap::default(),
            field_prims: Default::default(),
            field_values: FxHashMap::default(),
            field_tas: FxHashMap::default(),
            describing: Default::default(),
            ta_reported: Default::default(),
            stand_in_tas: Vec::new(),
            indexed_allocation_sites: FxHashMap::default(),
            cells: Vec::new(),
            cell_bad: Default::default(),
            cell_flows: Default::default(),
            cell_flow_toks: Default::default(),
            cells_read: Vec::new(),
            cell_seeds: FxHashMap::default(),
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
            reach_touch: Vec::new(),
            stand_in_cc: FxHashMap::default(),
            own_classes: FxHashMap::default(),
            closed_ctors: Vec::new(),
            effects_done: Default::default(),
            effect_site: None,
            effect_srcs: Default::default(),
            effect_bad: Default::default(),
            binding_writes: FxHashMap::default(),
            for_binding: None,
            narrow: Vec::new(),
            narrow_scope: None,
            narrow_consts: Default::default(),
            call_args_at: Vec::new(),
            call_results_at: Vec::new(),
            call_sites_at: Vec::new(),
            str_lits: Default::default(),
            for_of_walks: Default::default(),
            initialized_consts: Default::default(),
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
            fn_store_top: Default::default(),
            unit_in: None,
            view: None,
            stand_ins: FxHashMap::default(),
            ctor_phase: FxHashMap::default(),
            fn_inst_of: FxHashMap::default(),
            field_init_fn: FxHashMap::default(),
            static_init_fn: FxHashMap::default(),
            prim_const,
            num_lits: FxHashMap::default(),
            walked: Default::default(),
            pending_walk: Default::default(),
            shared_closures: Default::default(),
            class_envs: FxHashMap::default(),
            export_sources: Default::default(),
            dynimport_sabotage: false,
            int_ranges: true,
            arr_lengths: true,
            new_array_site: None,
            push_fill: Default::default(),
            guard_ranges: true,
            guard_presence: true,
            leaving_writes: true,
            zs_ok: false,
            leaving_write_sabotage: false,
            jumps: 0,
            tries: 0,
            dynimport_seen: Default::default(),
            dynamic_only: Default::default(),
            roots: Vec::new(),
            init_fn_of: FxHashMap::default(),
            once_ids: Vec::new(),
            order_ok: false,
            loop_depth: 0,
            iife_birth: FxHashMap::default(),
            fn_phase: FxHashMap::default(),
            rooted_stores: Default::default(),
            class_homes: FxHashMap::default(),
            chunk_objects: Vec::new(),
            cell_locals: Default::default(),
            lit_births: FxHashMap::default(),
            default_args: FxHashMap::default(),
            call_lit_arg: None,
            own_reads: FxHashMap::default(),
            own_store_reads: Default::default(),
            own_test_recv: None,
            for_in_keys_of: FxHashMap::default(),
            for_in_key: Vec::new(),
            const_inits: FxHashMap::default(),
            unstable: FxHashMap::default(),
            param_calls: FxHashMap::default(),
        }
    }

    // ------------------------------------------------------------ registry

    /// Register every module's functions, classes and scope before any body
    /// is walked, so cross-references resolve in one pass.
    pub fn register_all(&mut self) {
        self.index_emitted_closures();
        self.note_const_inits();
        self.note_indexed_allocation_sites();
        let modules = self.modules.clone();
        for (mi, (path, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            let ns_tok = self.m.tok(TokKind::ModNs(mi));
            self.mods.push(ModInfo {
                path: path.to_string(),
                locals: FxHashMap::default(),
                dead_zones: FxHashMap::default(),
                globals: FxHashMap::default(),
                funcs: FxHashMap::default(),
                classes: FxHashMap::default(),
                imports: FxHashMap::default(),
                scope: FxHashMap::default(),
                ns_tok,
                site_counter: 0,
                iterres: FxHashMap::default(),
                iterout: FxHashMap::default(),
                stepyield: FxHashMap::default(),
            });
            for f in self.functions_of(mi as usize) {
                let kind = Some(perry_hir::fn_proto::FnProtoKind::of_function(
                    self.modules[mi as usize].1,
                    f,
                ));
                let fx = self.register_fn(mi, f.id, &f.name, &f.params, false, false, kind);
                self.m.decls.insert(fx);
            }
            for c in self.classes_of(mi as usize) {
                self.register_class(mi, c);
            }
            // Closures are registered when the walker meets them.
            let _ = module;
        }
        // Keys a for-in body may define, before any lookup is generated.
        self.note_for_in_define_keys();
        // Keys a definition names by a constant, before any lookup too.
        self.note_define_keys();
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
                // A module outside the program that is no builtin is unseen
                // code run at load: it may write any key of the global
                // object (rule G reads an unresolved global as `native_written`
                // then). In per-unit mode a missing target is another unit.
                if target.is_none()
                    && !self.m.unit_mode
                    && !crate::builtins::is_builtin_module(&imp.source)
                {
                    let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                    let what = format!("module {} runs unseen", imp.source);
                    crate::builtins::note_native_write(&mut self.m, t, None, &what);
                }
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
        // Explain: why this unit reports each contract it consumed.
        if self.m.explain && !self.m.aborted {
            let mut why: Vec<String> = Vec::new();
            for (d, (_, tm, toks)) in &self.stand_ins {
                if !out.contains(d) {
                    continue;
                }
                let mut rs: Vec<String> = Vec::new();
                if self.m.foreign_written.contains(tm) {
                    rs.push("written".into());
                }
                if self.m.adds_keys.get(tm).is_some_and(|&k| k > 0) {
                    rs.push("adds keys".into());
                }
                for &t in toks {
                    if let Some(&r) = self.m.escaped.get(&t) {
                        rs.push(format!("escaped: {}", self.m.reasons[r as usize]));
                    }
                    if let Some(&(_, r)) = self.m.hard.get(&t) {
                        rs.push(format!("hard: {}", self.m.reasons[r as usize]));
                    }
                }
                why.push(format!("{d}: {}", rs.join("; ")));
            }
            why.sort();
            let unit = self.view.map_or("", |v| v.key.as_str());
            for w in why {
                eprintln!("perry: wpfacts explain: unit {unit} reports {w}");
            }
        }
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
            // DECISIONS 210: used as an object beyond the contract.
            if escaped || self.m.hard.contains_key(&t) || self.m.fn_touched.contains(&t) {
                out.push(format!("fn:{name}"));
            }
        }
        // A value stored under a named key of it is one the owner does not
        // see (an element store is the typed array's own data).
        for (rep, t) in &self.stand_in_tas {
            let written = self
                .m
                .tmpl_of(*t)
                .is_none_or(|tm| crate::bindings::owner_metadata_written(&self.m, tm));
            if self.m.aborted || written || self.m.is_escaped(*t) || self.m.hard.contains_key(t) {
                if self.m.explain {
                    let esc = self
                        .m
                        .escaped
                        .get(t)
                        .map(|&r| self.m.reasons[r as usize].clone());
                    let hard = self
                        .m
                        .hard
                        .get(t)
                        .map(|&(_, r)| self.m.reasons[r as usize].clone());
                    eprintln!(
                        "perry: wpfacts explain: unit {} reports {rep}: written {written} escaped {esc:?} hard {hard:?}",
                        self.view.map_or("", |v| v.key.as_str())
                    );
                }
                out.push(rep.clone());
            }
        }
        out.extend(self.override_reports());
        // A class of another unit this unit gave as a class object's parent
        // (`Object.setPrototypeOf(B, A)`, `B.__proto__ = A`): a `super()`
        // runs its constructor on a receiver its unit does not see, as for
        // a class whose `extends` names it (DECISIONS 57). Not a break: its
        // unit publishes the constructor contract.
        for (d, (_, tm, _)) in &self.stand_ins {
            if self.m.relinked_parents.contains(tm) {
                out.push(format!("extends {d}"));
            }
        }
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
            // DECISIONS 234: a key of its instances may disappear or be
            // redefined (delete, defineProperty): no key of theirs is sure.
            let mut open = tp.has_computed_members || m.keys_unsure.contains(&c);
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
    /// DECISIONS 224: a function published to a known closed callee is not
    /// open: its owner runs it with what its runners pass beside the
    /// markers (`FnContract::callable`), so unknown code holding it is this
    /// unit's report, which breaks it (its parameters are then ⊤).
    pub(crate) fn open_fn(&self, t: Tok) -> bool {
        self.stand_in_fc
            .get(&t)
            .is_some_and(|c| c.closed.is_none() && !c.callable && c.lexical_this)
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

    /// The keys a class method is found under. HIR names a well-known-symbol
    /// method `@@<name>` (`[Symbol.iterator]()` is `@@iterator`); a read
    /// names the symbol `@@__perry_wk_<name>` (`Gen::read_key`). Both: the
    /// runtime binds the method to the symbol, and the string key stays
    /// what HIR registered.
    fn method_keys(&mut self, name: &str) -> Vec<Atom> {
        let mut v = vec![self.m.atom(name)];
        if let Some(wk) = name.strip_prefix("@@") {
            if !name.starts_with(crate::builtins::WK_PREFIX) {
                v.push(self.m.atom(&format!("{}{wk}", crate::builtins::WK_PREFIX)));
            }
        }
        v
    }

    pub(crate) fn register_fn(
        &mut self,
        mi: u32,
        id: u32,
        name: &str,
        params: &[perry_hir::Param],
        lexical_this: bool,
        is_closure: bool,
        proto: Option<perry_hir::fn_proto::FnProtoKind>,
    ) -> FnIdx {
        if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
            return f;
        }
        if is_closure {
            if let Some(&reload) = self.once_ids.get(mi as usize).and_then(|o| o.get(&id)) {
                let f = self.m.funcs.len() as FnIdx;
                self.m.once_fns.insert(f, (mi, reload));
            }
        }
        let fx = self.m.funcs.len() as FnIdx;
        #[cfg(test)]
        let packet_prior = self.m.packet_header(
            fx,
            crate::summary::packet::Identity(self.modules[mi as usize].0.into(), id, String::new()),
            params,
        );
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        if is_closure {
            self.m.closures.insert(fx);
        }
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        // Predicate evidence is global: a bad closure context invalidates
        // all callers, even when its body is summarized.
        let own_test = self.m.node_l("own-test");
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
            // `arguments` beside a user rest parameter is no rest itself
            // (the HIR appends it after `...r`), but the same object: every
            // argument, never a positional one.
            if p.is_rest || p.arguments_object.is_some() {
                // The rest local holds a fresh array whose elements are the
                // extra arguments (and, for `arguments`, every argument).
                let arr = match &p.arguments_object {
                    Some(_) => self.new_args_template(mi),
                    None => self.new_arr_template(mi, "rest"),
                };
                let at = self.m.tok(TokKind::Obj(arr, NO_CTX));
                self.m.add_tok(n, at);
                let fa = self.m.field_any(at);
                if let Some(ao) = &p.arguments_object {
                    self.m.edge(args_all, fa);
                    // Its own `length` is the argument count until code
                    // stores another value; sloppy `callee` is the function
                    // (strict `callee` is an accessor that throws).
                    let la = self.m.atom("length");
                    let lf = self.m.field(at, la);
                    self.m.add_prim(lf, prim::NUM);
                    if !ao.strict {
                        let ca = self.m.atom("callee");
                        let cf = self.m.field(at, ca);
                        self.m.add_tok(cf, tok);
                    }
                    // Sloppy mapped `arguments` aliases the params: writes
                    // through it reach them (E24). The synthetic parameter
                    // may come before the named ones, so the edges are added
                    // once every parameter is known.
                    if p.arguments_object.as_ref().is_some_and(|a| !a.strict) {
                        mapped = Some((fa, self.m.all_fields(at)));
                    }
                }
                if p.is_rest {
                    rest = Some(n);
                }
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
                if p.default.is_some() {
                    // A defaulted parameter: the calls pass to its own
                    // argument node; the local is that value without
                    // `undefined`, plus the default (`params_inner`).
                    let a = self.m.node_named("arg", || format!("arg({name})"));
                    self.m.own(a);
                    self.default_args.insert((mi, p.id), a);
                    ps.push(a);
                } else {
                    ps.push(n);
                }
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
            #[cfg(test)]
            body_packet: Default::default(),
            module: mi,
            func_id: id,
            raise: u32::MAX,
            name: format!("{}:{}", self.mods[mi as usize].path_short(), name),
            params: ps,
            rest,
            this,
            ret,
            args_all,
            has_arguments: params.iter().any(|p| p.arguments_object.is_some()),
            own_test: OwnTestBody::Pending(own_test),
            token: tok,
            ctor_of: None,
            lexical_this,
            no_summary: false,
            gen_proto: None,
            gen_fn_protos: None,
            gen_async: false,
            gen_reyields: false,
            proto,
            captures: Vec::new(),
            forwards_super: false,
            parent: if is_closure { outer_fn } else { NO_FN },
            forwarded: false,
        });
        #[cfg(test)]
        self.m.packet_end(packet_prior, false);
        self.mods[mi as usize].funcs.insert(id, fx);
        fx
    }

    fn register_class(&mut self, mi: u32, c: &Class) {
        let module = self.modules[mi as usize].1;
        let tm = self.m.tmpls.len() as TmplId;
        let display = format!("{}:{}", self.mods[mi as usize].path_short(), c.name);
        let mut accessor_keys = Vec::new();
        for (n, _) in c.getters.iter().chain(c.setters.iter()) {
            let a = self.accessor_atom(mi, c, n);
            if !accessor_keys.contains(&a) {
                accessor_keys.push(a);
            }
        }
        let named: Vec<Option<(Atom, bool)>> = c
            .computed_members
            .iter()
            .map(|cm| self.named_computed(mi, cm))
            .collect();
        for &(a, _) in named.iter().flatten() {
            push_unique(&mut accessor_keys, a);
        }
        let proto_members = {
            let mut v = accessor_keys.clone();
            for meth in &c.methods {
                let a = self.m.atom(&meth.name);
                push_unique(&mut v, a);
            }
            // A literal has no own `constructor`: it reads
            // `Object.prototype`'s, which a store into ⊤ may replace.
            if !c.is_literal_shape() {
                let a = self.m.atom("constructor");
                push_unique(&mut v, a);
            }
            v
        };
        if !c.is_literal_shape() {
            self.own_classes.entry(display.clone()).or_insert(tm);
        }
        // An object literal's shape: every birth evaluates all its values
        // first and defines every key before the object can be seen, so its
        // keys are present (the fields of a class are not: its constructor
        // can read them before they are stored).
        let literal_keys: Vec<Atom> = if c.is_literal_shape() {
            c.fields
                .iter()
                .filter(|f| f.key_expr.is_none())
                .map(|f| self.m.atom(&f.name))
                .collect()
        } else {
            Vec::new()
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
            definite: literal_keys,
            declared: Vec::new(),
            reserved: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: c.native_extends.clone(),
            accessor_keys: accessor_keys.clone(),
            has_computed_members: named.iter().any(|n| n.is_none()),
            field_init: None,
            proto_members,
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: c.declared_keys_null_proto(),
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
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
            let f = self.register_fn(mi, ctor.id, &ctor.name, &ctor.params, false, false, None);
            self.m.funcs[f as usize].ctor_of = Some(tm);
            self.m.funcs[f as usize].forwards_super = forwards_super(ctor);
            self.m.tmpls[tm as usize].ctor = Some(f);
            // Only an environment-publishing constructor takes its hidden
            // arguments from that environment. Instance captures forward the
            // actual argument, which their ClassCaptureValue rebind prefers
            // over the declaration snapshot (including an unseeded snapshot).
            let publishes_environment = ctor.body.iter().any(|s| {
                matches!(s, perry_hir::Stmt::Expr(perry_hir::Expr::ClassEnvSet {
                    class_name, publish: true, ..
                }) if class_name == &c.name)
            });
            let caps = ctor
                .params
                .iter()
                .rev()
                .take_while(|p| perry_hir::cap_fields::is_cap_field_name(&p.name))
                .count();
            if publishes_environment {
                self.m.funcs[f as usize].captures =
                    (0..caps).map(|i| self.class_env(mi, &c.name, i)).collect();
            }
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
            let f = self.register_fn(
                mi,
                meth.id,
                &name,
                &meth.params,
                false,
                false,
                Some(perry_hir::fn_proto::FnProtoKind::of_function(module, meth)),
            );
            let t = self.m.funcs[f as usize].token;
            for a in self.method_keys(&meth.name) {
                let n = self.m.field(proto, a);
                self.m.add_tok(n, t);
            }
            let this = self.m.funcs[f as usize].this;
            self.m.this_class.insert(this, tm);
        }
        for meth in &c.static_methods {
            let name = format!("{}::static {}", c.name, meth.name);
            let f = self.register_fn(
                mi,
                meth.id,
                &name,
                &meth.params,
                false,
                false,
                Some(perry_hir::fn_proto::FnProtoKind::of_function(module, meth)),
            );
            let t = self.m.funcs[f as usize].token;
            for a in self.method_keys(&meth.name) {
                let n = self.m.field(cobj, a);
                self.m.add_tok(n, t);
                self.m.tmpls[tm as usize].static_methods.push(a);
            }
        }
        for (name, g) in &c.getters {
            let fname = format!("{}::get {name}", c.name);
            let f = self.register_fn(
                mi,
                g.id,
                &fname,
                &g.params,
                false,
                false,
                Some(perry_hir::fn_proto::FnProtoKind::of_function(module, g)),
            );
            // A static accessor is an own property of the class object;
            // its `this` is whatever reads it (a subclass, `Object.create`).
            let holder = if c.is_static_accessor(g) {
                cobj
            } else {
                let this = self.m.funcs[f as usize].this;
                self.m.this_class.insert(this, tm);
                proto
            };
            let a = self.accessor_atom(mi, c, name);
            self.m.add_accessor_atom(a);
            let ft = self.m.funcs[f as usize].token;
            self.m.accessors.entry((holder, a)).or_default().0.push(ft);
            push_unique(self.m.accessor_keys.entry(holder).or_default(), a);
        }
        for (name, s) in &c.setters {
            let fname = format!("{}::set {name}", c.name);
            let f = self.register_fn(
                mi,
                s.id,
                &fname,
                &s.params,
                false,
                false,
                Some(perry_hir::fn_proto::FnProtoKind::of_function(module, s)),
            );
            // A static accessor is an own property of the class object;
            // its `this` is whatever reads it (a subclass, `Object.create`).
            let holder = if c.is_static_accessor(s) {
                cobj
            } else {
                let this = self.m.funcs[f as usize].this;
                self.m.this_class.insert(this, tm);
                proto
            };
            let a = self.accessor_atom(mi, c, name);
            self.m.add_accessor_atom(a);
            let ft = self.m.funcs[f as usize].token;
            self.m.accessors.entry((holder, a)).or_default().1.push(ft);
            push_unique(self.m.accessor_keys.entry(holder).or_default(), a);
        }
        for (cm, named) in c.computed_members.iter().zip(named) {
            let f = self.register_fn(
                mi,
                cm.function.id,
                &format!("{}::{}#{}", c.name, cm.function.name, cm.function.id),
                &cm.function.params,
                false,
                false,
                Some(perry_hir::fn_proto::FnProtoKind::of_function(
                    module,
                    &cm.function,
                )),
            );
            if let Some((a, setter)) = named {
                // Under a constant key: that key's accessor, on the class
                // object when it is static.
                let holder = if cm.is_static {
                    cobj
                } else {
                    let this = self.m.funcs[f as usize].this;
                    self.m.this_class.insert(this, tm);
                    proto
                };
                self.m.add_accessor_atom(a);
                let ft = self.m.funcs[f as usize].token;
                let e = self.m.accessors.entry((holder, a)).or_default();
                if setter { &mut e.1 } else { &mut e.0 }.push(ft);
                push_unique(self.m.accessor_keys.entry(holder).or_default(), a);
                continue;
            }
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
                #[cfg(test)]
                if self.m.packets_enabled {
                    std::rc::Rc::make_mut(&mut self.m.funcs[f as usize].body_packet)
                        .header
                        .shared
                        .push(crate::summary::packet::SharedEntry::ComputedAccessor {
                            class: self.m.tmpls[tm as usize].display.clone(),
                        });
                }
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
        #[cfg(test)]
        let packet_prior = self.m.packet_header(
            fx,
            crate::summary::packet::Identity(
                self.modules[mi as usize].0.into(),
                u32::MAX,
                name.into(),
            ),
            &[],
        );
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        self.m.funcs.push(FuncInfo {
            #[cfg(test)]
            body_packet: Default::default(),
            module: mi,
            func_id: u32::MAX,
            raise: u32::MAX,
            name: format!("{}:{}", self.mods[mi as usize].path_short(), name),
            params: vec![],
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: false,
            own_test: OwnTestBody::Unknown,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: false,
            gen_proto: None,
            gen_fn_protos: None,
            gen_async: false,
            gen_reyields: false,
            proto: None,
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded: false,
        });
        #[cfg(test)]
        self.m.packet_end(packet_prior, false);
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
        // The name a member heritage carries (`extends O.Base`, `O["Base"]`)
        // is its property key, which names no binding: the parent is the
        // value the heritage evaluated to, whatever class elsewhere carries
        // that name (§17.2). A global name (`extends Error`) is a read of
        // the global object (`GlobalGet(0)`), a binding.
        let member = match c.extends_expr.as_deref() {
            Some(perry_hir::Expr::PropertyGet { object, .. }) => {
                !matches!(object.as_ref(), perry_hir::Expr::GlobalGet(0))
            }
            Some(perry_hir::Expr::IndexGet { .. } | perry_hir::Expr::StaticFieldGet { .. }) => true,
            _ => false,
        };
        let binding = c.extends_name.as_deref().filter(|_| !member);
        let parent = binding.and_then(|n| self.resolve_class_name(mi, n));
        let builtin_parent =
            binding.filter(|n| parent.is_none() && crate::builtins::native_parent_summarized(n));
        if let Some(p) = parent {
            self.m.tmpls[tm as usize].parent = Some(p);
        } else if let Some(n) = builtin_parent {
            // The binding's module when HIR resolved it to an import of a
            // node module (`import { EventEmitter } from "events"`), else
            // the global object's built-in.
            let module = c
                .native_extends
                .as_ref()
                .filter(|(_, name)| name == n)
                .map_or("builtin", |(m, _)| m.as_str());
            self.m.tmpls[tm as usize].native_parent = Some((module.into(), n.to_string()));
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
            self.parent_values
                .push((mi, tm, binding.map(str::to_string)));
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
        let native = tm.and_then(|tm| crate::builtins::native_parent_value(&mut self.m, tm));
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
            // A class linked by name to a built-in parent (`extends Error`):
            // `super()` runs the evaluated `extends` value with `this` = the
            // instance. The native summary (`native_parent_ctor`) is that run
            // only when the value is the constructor the global object
            // declares under the name; whether it is depends on what the
            // solve finds (a program or an unseen module may write the
            // global, rule G), and anything else runs before the solve ends,
            // so the value is checked during the solve (`Cons::NativeParent`).
            None if native.is_some() => {
                let tm = tm.unwrap();
                // The evaluated value is a built-in constructor, no class:
                // no static construction through a parent is published
                // (`super_unrelinked`); `super()` follows the live edge,
                // which is where the native run happens.
                self.m.parent_unsettled.insert(tm);
                let chk = self.m.node_l("native-parent-check");
                self.m.edge(v, chk);
                // The class object's [[Prototype]] is the evaluated `extends`
                // value (ClassDefinitionEvaluation), as for any parent value:
                // a static read or call on the class (`X.g()`) reaches what
                // the program stored on that constructor.
                let c = self.m.tok(TokKind::ClassObj(tm));
                let cp = self.m.proto_node(c);
                self.m.edge(v, cp);
                let reason = self.m.reason(why);
                self.m.cons(
                    chk,
                    Cons::NativeParent {
                        tm,
                        expected: native.unwrap(),
                        reason,
                    },
                );
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

    /// The key accessor `name` of class `c` is held under: a private one
    /// (`get #x()`) under the dispatch key its guarded access reads
    /// (`Expr::PrivateGuard` keeps the `perry_hir::private_names` key), named
    /// by the class it was specialized from, as its fields are.
    fn accessor_atom(&mut self, mi: u32, c: &Class, name: &str) -> Atom {
        if !name.starts_with('#') {
            return self.m.atom(name);
        }
        let k = perry_hir::private_names::private_member_key(self.private_class_id(mi, c), name);
        self.m.atom(&k)
    }

    /// The class whose private elements class `c` of module `mi` carries:
    /// the one it was specialized from (codegen `element_class_id_in`).
    fn private_class_id(&self, mi: u32, c: &Class) -> u32 {
        let module = self.modules[mi as usize].1;
        let mut cur = c;
        for _ in 0..module.classes.len() {
            match cur
                .specialized_from
                .as_deref()
                .and_then(|o| module.classes.iter().find(|k| k.name == o))
            {
                Some(o) => cur = o,
                None => break,
            }
        }
        cur.id
    }

    /// The storage key of private field `name` (`#x`) declared by class `c`
    /// of module `mi`: the class its private elements belong to, the one it
    /// was specialized from (codegen `element_class_id_in`), names it.
    pub(crate) fn private_field_key(&self, mi: u32, c: &Class, name: &str) -> String {
        perry_hir::private_names::private_value_key(self.private_class_id(mi, c), name)
    }

    /// The key of static field `field_name` of class `class_name`: a `#x`
    /// is the class's private static (codegen `static_field_meta`).
    pub(crate) fn static_field_atom(
        &mut self,
        mi: u32,
        class_name: &str,
        field_name: &str,
    ) -> Atom {
        if field_name.starts_with('#') {
            let module = self.modules[mi as usize].1;
            if let Some(c) = module.classes.iter().find(|c| c.name == class_name) {
                let key = self.private_field_key(mi, c, field_name);
                return self.m.private_atom(&key);
            }
        }
        self.m.atom(field_name)
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

    /// What a local's cell holds before its declaration runs: the TDZ seed
    /// (`Stmt::PreallocateTdzBoxes`) writes `undefined` here, not into the
    /// local. The seed is not a value of the binding: a program read that
    /// finds it throws a ReferenceError (`js_box_get_bits`), so only
    /// initialized values flow out of a read. Only the compiler's own
    /// capture snapshots read a cell in its dead zone without throwing
    /// (`js_tdz_suppress_begin`): they take `undefined` (`snapshot_read`).
    pub(crate) fn dead_zone(&mut self, mi: u32, id: u32) -> NodeId {
        if let Some(&n) = self.mods[mi as usize].dead_zones.get(&id) {
            return n;
        }
        let n = self
            .m
            .node_named("dead zone", || format!("dead_zone({mi},{id})"));
        self.mods[mi as usize].dead_zones.insert(id, n);
        n
    }

    /// A class capture snapshot's read of `e` (codegen brackets it in a
    /// TDZ-suppression window): what the read sees, and when `e` reads a
    /// local, the `undefined` its cell holds in the dead zone.
    pub(crate) fn snapshot_read(&mut self, fx: &Fx, e: &perry_hir::Expr) -> NodeId {
        let v = self.expr(fx, e);
        let perry_hir::Expr::LocalGet(id) = e else {
            return v;
        };
        let z = self.dead_zone(fx.module, *id);
        let out = self.tmp("capture snapshot");
        self.m.edge(v, out);
        self.m.edge(z, out);
        out
    }

    pub(crate) fn local(&mut self, mi: u32, id: u32) -> NodeId {
        if let Some(n) = self.for_in_key_param(mi, id) {
            return n;
        }
        if let Some(name) = self.view.and_then(|v| v.foreign_locals.get(&id)) {
            return self.extern_value(mi, name);
        }
        if let Some(&n) = self.mods[mi as usize].locals.get(&id) {
            #[cfg(test)]
            self.m.packet_cell(n, id);
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
        #[cfg(test)]
        self.m.packet_cell(n, id);
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
        self.new_arr_birth(mi, what, false, false)
    }

    /// An array born with every index below its length (`TmplKind::Arr`
    /// `hole_free`) or not. `plain`: a program literal or spread literal
    /// (`TmplKind::Arr::plain`), set only with `Model::array_reps` on.
    pub(crate) fn new_arr_birth(
        &mut self,
        mi: u32,
        what: &str,
        hole_free: bool,
        plain: bool,
    ) -> TmplId {
        let site = self.next_site(mi);
        let kind = TmplKind::Arr {
            module: mi,
            site,
            hole_free,
            plain: plain && self.m.array_reps,
        };
        self.new_indexed_template(mi, kind, what, site)
    }

    /// A function's `arguments` object: not an Array (`TmplKind::Args`).
    pub(crate) fn new_args_template(&mut self, mi: u32) -> TmplId {
        let site = self.next_site(mi);
        self.new_indexed_template(mi, TmplKind::Args { module: mi, site }, "arguments", site)
    }

    fn new_indexed_template(
        &mut self,
        mi: u32,
        kind: TmplKind,
        what: &str,
        site: SiteId,
    ) -> TmplId {
        let tm = self.m.tmpls.len() as TmplId;
        self.note_birth_fn(tm);
        self.m.tmpls.push(Template {
            kind,
            display: format!("{}:[{what}#{site}]", self.mods[mi as usize].path_short()),
            birth_keys: vec![],
            definite: vec![],
            declared: Vec::new(),
            reserved: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
        });
        tm
    }

    /// A generation-time birth template belongs to the body making it.
    pub(crate) fn note_birth_fn(&mut self, tm: TmplId) {
        #[cfg(test)]
        if let Some(b) = &mut self.m.packet_scope {
            b.packet.births.push(crate::summary::packet::Birth {
                template: tm,
                module: String::new(),
                kind: None,
                keys: Vec::new(),
            });
        }
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
            reserved: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
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
    // The module's path: a display names a function, class or array across
    // units (one name, one contract, DECISIONS 106), so it names its module.
    // A shortened one did not: two versions of a package (jose@6.0.11 and
    // jose@6.2.9: `jose/dist/webapi/jwks/local.js`), or files of one name in
    // two directories (`instructions.ts`), published one function contract
    // name twice, and its readers saw one owner's contract and the other's
    // at the end of every round (opencode: 13 units solved in every round).
    p.to_string()
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
