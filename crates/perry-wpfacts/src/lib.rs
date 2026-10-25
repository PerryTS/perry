//! Whole-program invariance facts (wpfacts), slice 1.
//!
//! An inclusion-based (Andersen-style) value-flow analysis over the final
//! HIR of every module, failing closed: anything it does not model flows to
//! one ⊤ node. It computes, per birth template (class, object-literal
//! shape, constructor-function body):
//!
//! - F1 writers / readonly-after-construction fields,
//! - F1′ definite birth keys,
//! - F2 field value kinds,
//! - F3 escape (E1–E25, B1–B4), prefix-stable and fixed-layout,
//! - F4 receiver/parameter class sets (as site verdicts),
//! - F5 single method targets.
//!
//! Slice 1 removes no check: codegen consumes only the per-module digest
//! (object-cache key) and, in contract builds, the claims section. See
//! `secret-tests/scratchpad/wpfacts-design-20261012.md`.

pub mod bindings;
mod builtins;
mod ctor_phase;
mod decl_units;
pub mod delta;
pub mod derive;
mod dump;
mod gen;
mod gen_call;
mod gen_cjs;
mod gen_const;
mod gen_expr;
mod gen_rest;
pub mod model;
pub mod pts;
mod rooted;
mod solve;
mod summary;
#[cfg(test)]
mod tests;
mod units;

pub use bindings::{BindingFacts, BodyRef, ModuleBindings};
pub use derive::{FieldFacts, KindFact, SiteFacts, TemplateFacts, Verdict};
use perry_hir::Module;

/// Reserved atom for computed-key accessors ("any key").
pub(crate) const STAR_KEY: &str = "\u{1}*";

#[derive(Clone, Debug, Default)]
pub struct FactsConfig {
    /// Record the constraint relations for the independent recomputation.
    pub relations: bool,
    /// Test-only negative control: plant a wrong fact
    /// (`readonly:T.f`, `prefix_stable:T`, `fixed_layout:T`,
    /// `kind:T.f=num|str|bool|obj`, `definite:T.f`).
    pub sabotage: Option<String>,
    /// Wall-clock budget (seconds) on generation and solve; past it the
    /// program gets no facts.
    pub budget_secs: Option<u64>,
    /// Per-constraint-kind time profile in the report.
    pub profile: bool,
    /// Bound on points-to entries (work/memory); past it, no facts.
    pub max_pts: Option<u64>,
    /// Deterministic solve-work bound (token propagations along edges and
    /// into constraints); past it, no facts. Unlike the wall budget it
    /// gives the same answer on every machine.
    pub max_work: Option<u64>,
    /// The driver knows the program can run interpreted code (a deferred
    /// `new Function`/`eval` site or `node:vm`). Not a kill (decision 150):
    /// interpreted code is the ⊤ summary node; see
    /// [`ProgramFacts::interp_guard`].
    pub dynamic_code: Option<String>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    /// Diagnostics: name where ⊤ came from in each ⊤ site verdict.
    pub explain: bool,
    /// The driver's resolution of every `export … from "src"` source: a
    /// re-export has no import of its source to resolve it by.
    pub export_sources: ExportSources,
    /// The modules are one unit of a larger program: every other unit is
    /// unknown compiled code (`Gen::unit_boundary`).
    pub unit: bool,
    /// Decision 32: threads for the per-unit driver's dependency levels
    /// (0 and 1 solve on the calling thread). The facts are the same at
    /// every count.
    pub jobs: usize,
    /// Per-unit inputs/outputs digests in the dump (`# uio` / `# uout`):
    /// what an edit changes, unit by unit (design §13.3).
    pub unit_io: bool,
}

/// Module path → `export … from` source → the module path it resolves to.
pub type ExportSources =
    std::collections::HashMap<String, std::collections::HashMap<String, String>>;

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub modules: usize,
    pub templates: usize,
    pub fact_units: usize,
    pub nodes: usize,
    pub constraints: u64,
    pub tokens: usize,
    pub functions: usize,
    pub gen_ms: f64,
    pub solve_ms: f64,
    pub derive_ms: f64,
    pub pops: u64,
    /// Summary instances, the shared ones among them, and calls expanded
    /// into their caller's summary.
    pub instances: u64,
    pub shared_instances: u64,
    pub composed: u64,
    /// Closure contexts made (design §14.2).
    pub contexts: u64,
    /// Solve work done (see `FactsConfig::max_work`).
    pub work: u64,
    /// The solve hit its budget: no facts were claimed.
    pub aborted: bool,
    /// Contract checks run after the solve (§11.2): none past the bound.
    pub checks: u64,
    pub top_calls: u64,
    /// (constraint kind, applications, ms), most expensive first.
    pub profile: Vec<(String, u64, f64)>,
    pub pts_entries: u64,
    /// The largest points-to sets: (node label, size, token-kind mix).
    pub big_nodes: Vec<(String, usize, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct ProgramFacts {
    pub module_paths: Vec<String>,
    pub templates: Vec<TemplateFacts>,
    pub sites: Vec<SiteFacts>,
    pub stats: Stats,
    /// Interpreted code can run (decision 150), and why. It is no kill: its
    /// data flow is the ⊤ summary node (everything it can receive already
    /// escapes there). What the analysis cannot see is interpreted code
    /// writing an intrinsic prototype (an accessor or method under any key
    /// on `Object.prototype`, `Array.prototype` …): that runs it with the
    /// receiver of any lookup reaching that prototype, and a setter there
    /// keeps a constructor's assignment from defining its birth field. The
    /// runtime records exactly that (`PERRY_INTERP_PROTO_WRITTEN`, set-only),
    /// so with this set every proof is GUARDED: the site tests the word and
    /// takes its complete generic operation once it is set.
    pub interp_guard: Option<String>,
    /// Compiled code may write `%Function.prototype%.bind` (or the
    /// `name`/`length` bind reads), and why; `None`: it is the builtin's own
    /// data property for the whole run as far as compiled code goes (an
    /// aborted solve answers `Some`). Interpreted code is
    /// [`ProgramFacts::interp_guard`]'s.
    pub fnproto_bind_written: Option<String>,
    /// Constructors whose live [[Prototype]] cannot be relinked by any reachable operation.
    pub super_unrelinked: std::collections::BTreeSet<u32>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    pub sabotage: Option<String>,
    pub relations: Option<Vec<String>>,
    pub tok_names: Vec<String>,
    /// Receiver facts per binding, per module (slice 2: codegen elision).
    pub bindings: Vec<ModuleBindings>,
    /// Per-unit mode: each unit's path and statistics.
    pub units: Vec<(String, Stats)>,
    /// `FactsConfig::unit_io`: the `# uio` / `# uout` dump lines.
    pub unit_io: Vec<String>,
    /// Per-unit mode: driver rounds (1 + census re-solves) and the classes
    /// the census broke (design §11.3).
    pub rounds: u32,
    pub broken: Vec<String>,
    /// Solve work of every unit, per round (`stats.work` is their sum).
    pub round_work: Vec<u64>,
    /// Per-unit mode: published contracts the driver compared between
    /// rounds, over every round (DECISIONS 114: once per contract, not per
    /// reading unit).
    pub compares: u64,
    /// One unit's class contracts and census reports (design §11).
    pub(crate) contracts_out: Vec<((String, String), units::ClassContract)>,
    pub(crate) fn_contracts_out: Vec<((String, String), units::FnContract)>,
    /// The census display (`fn:<contract name>`) of every function of this
    /// unit some contract may describe (`contract_fns`): whatever path
    /// published it (export, literal field, closed argument), a report
    /// against it is a report against this unit (`units::owners`).
    pub(crate) fns_defined: Vec<String>,
    pub(crate) layouts_out: Vec<units::ClassContract>,
    pub(crate) values_out: Vec<((String, String), units::ValueContract)>,
    /// What its calls pass to other units' closed functions (decision 13).
    pub(crate) args_out: Vec<(String, units::ArgContract)>,
    /// What it passes to closed classes' constructors (design §18.13), by
    /// the class's module path and name (`ClassContract::closed`).
    pub(crate) ctor_args_out: Vec<((String, String), units::ArgContract)>,
    /// Its store effects into other units' literal layouts and class
    /// instances (decision 17): display, key and the value contract of what
    /// it stores.
    pub(crate) stores_out: Vec<(String, String, units::ValueContract)>,
    /// Its writes of other units' module-scope bindings: (unit path, name)
    /// and the value contract of what it stores (`Gen::local_write`).
    pub(crate) writes_out: Vec<((String, String), units::ValueContract)>,
    /// What it writes into each cell (design §17.16 (b)), and the cells it
    /// reads (`Gen::seed_cell`).
    pub(crate) cells_out: Vec<(String, units::ValueContract)>,
    pub(crate) cells_read: Vec<String>,
    /// The layouts it named by display (`Gen::other_layout`, DECISIONS 103).
    pub(crate) layouts_read: Vec<(String, u64)>,
    /// Its own contract functions' contracts, by name (DECISIONS 106): a
    /// reader takes the owner's over any copy a cell or a forwarding
    /// unit's contract carries, and the functions it named so
    /// (`Gen::owner_fn`), read like a cell.
    pub(crate) own_fns_out: Vec<units::FnContract>,
    pub(crate) fns_read: Vec<(String, u64)>,
    /// DECISIONS 107: per method of another unit (contract name), the
    /// receivers this unit runs it on.
    pub(crate) this_args_out: Vec<(String, units::ValueContract)>,
    /// DECISIONS 165: the methods of other units (contract names) this
    /// unit runs.
    pub(crate) method_runs_out: Vec<String>,
    /// DECISIONS 148: per method of another unit it runs (contract name),
    /// what its calls pass; `top`: a call no contract describes (a stub
    /// method, a method reached under no name).
    pub(crate) method_args_out: Vec<(String, units::ArgContract)>,
    /// The displays in `layouts_out` that are other units' layouts (a
    /// stand-in's contract, forwarded): never over the owner's entry.
    pub(crate) layouts_fwd: Vec<String>,
    pub(crate) reports: Vec<String>,
    module_digests: Vec<u64>,
}

/// Run the analysis over every module (paths must be the keys the caller
/// uses for `digest_for` / `contract_section`).
pub fn compute(modules: &[(String, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    compute_in(modules, cfg, None, None)
}

/// A published class's static members along its chain inside the unit
/// (first one wins, as a lookup does): the key and the static method's
/// name when the member holds exactly one function; and whether the class
/// object may hold other keys (a computed static store, a native parent).
fn class_statics(g: &gen::Gen, tm: model::TmplId) -> (Vec<(String, Option<model::FnIdx>)>, bool) {
    let pa = g.m.atoms.iter().position(|x| x == "prototype");
    let mut out: Vec<(String, Option<model::FnIdx>)> = Vec::new();
    let mut open = false;
    let mut cur = Some(tm);
    while let Some(c) = cur {
        let Some(&cobj) = g.m.tok_index.get(&model::TokKind::ClassObj(c)) else {
            break;
        };
        if let Some(&an) = g.m.field_any.get(&cobj) {
            let node = &g.m.nodes[an as usize];
            open |= node.prim != 0 || !node.pts.is_empty();
        }
        open |= g.m.tmpls[c as usize].native_parent.is_some();
        let mut own: Vec<(String, Option<model::FnIdx>)> =
            g.m.field_keys
                .get(&cobj)
                .into_iter()
                .flatten()
                .filter(|&&k| Some(k as usize) != pa)
                .filter_map(|&k| {
                    let n = *g.m.fields.get(&(cobj, k))?;
                    let ts = g.m.nodes[n as usize].pts.to_sorted();
                    let f = ts.first().map(|&t| g.m.tok_fn(t)).filter(|&f| {
                        ts.len() == 1 && f != model::NO_FN && g.m.nodes[n as usize].prim == 0
                    });
                    Some((g.m.atoms[k as usize].clone(), f))
                })
                .filter(|(k, _)| !out.iter().any(|(o, _)| o == k))
                .collect();
        own.sort();
        out.extend(own);
        cur = g.m.tmpls[c as usize].parent;
    }
    (out, open)
}

/// The templates whose objects the defining unit stored into under a
/// computed key: a reader may find any key on them.
fn open_templates(g: &gen::Gen) -> pts::FxHashSet<model::TmplId> {
    g.m.field_any
        .iter()
        .filter(|&(_, &n)| {
            let node = &g.m.nodes[n as usize];
            node.prim != 0 || !node.pts.is_empty()
        })
        .filter_map(|(&h, _)| g.m.tmpl_of(h))
        .collect()
}

/// The fields of literal template `tm` (its tokens among `toks`) that hold
/// only contract functions of this unit (`Gen::note_field_fns`), each with
/// their function contracts.
fn literal_field_fns(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    toks: &[model::Tok],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, Vec<units::FnContract>)> {
    let mine: Vec<model::Tok> = toks
        .iter()
        .copied()
        .filter(|&t| g.m.tmpl_of(t) == Some(tm))
        .collect();
    let mut keys: Vec<model::Atom> = mine
        .iter()
        .flat_map(|t| g.m.field_keys.get(t).into_iter().flatten().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let mut out = Vec::new();
    'key: for a in keys {
        let mut fs: Vec<(model::FnIdx, model::Tok)> = Vec::new();
        for &t in &mine {
            let Some(&n) = g.m.fields.get(&(t, a)) else {
                continue;
            };
            if !g.field_fns.contains_key(&(t, a)) {
                continue 'key;
            }
            let node = &g.m.nodes[n as usize];
            if node.prim != 0 {
                continue 'key;
            }
            for ft in node.pts.to_sorted() {
                match g.m.toks[ft as usize] {
                    model::TokKind::Func(f, _)
                        if g.m.contract_fns.contains(&f) || g.open_fn(ft) =>
                    {
                        if !fs.iter().any(|x| x.0 == f) {
                            fs.push((f, ft));
                        }
                    }
                    _ => continue 'key,
                }
            }
        }
        if fs.is_empty() {
            continue;
        }
        fs.sort_unstable();
        // Another unit's open function forwards its own contract (§18.9).
        let fcs = fs
            .into_iter()
            .map(|(f, ft)| match g.stand_in_fc.get(&ft) {
                Some(&c) if g.m.funcs[f as usize].func_id == u32::MAX => c.clone(),
                _ => fn_contract_of(g, templates, f, layouts_out, open_tm),
            })
            .collect();
        out.push((g.m.atoms[a as usize].clone(), fcs));
    }
    out
}

/// The value contracts of literal template `tm`'s fields that forward
/// other units' stand-ins (DECISIONS 88, `Gen::nested_fields`): per key the
/// join over every instance that left.
fn field_value_contracts(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, units::ValueContract)> {
    let mut keys: Vec<(model::Atom, Vec<model::NodeId>)> = g
        .field_values
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), v)| (a, v.clone()))
        .collect();
    keys.sort_unstable();
    let mut out = Vec::new();
    for (a, ns) in keys {
        let mut vc = units::ValueContract::default();
        for n in ns {
            vc.join(&value_contract_of(
                g,
                templates,
                n,
                &[],
                layouts_out,
                open_tm,
            ));
        }
        out.push((g.m.atoms[a as usize].clone(), vc));
    }
    out
}

/// DECISIONS 126: every value the closed parameter node `p` holds (its
/// callers' described arguments) holds only stand-in typed arrays under key
/// `a`. Before any caller published (round 1) it holds nothing: bottom, as
/// decision 13's closed parameters start (DECISIONS 123); the callers'
/// arguments make it grow and the body is solved again.
fn described_ta_key(g: &gen::Gen, p: model::NodeId, a: model::Atom) -> bool {
    for t in g.m.nodes[p as usize].pts.to_sorted() {
        if g.m.param_marker.contains_key(&t) {
            continue;
        }
        let Some(&n) = g.m.fields.get(&(t, a)) else {
            return false;
        };
        let ts = g.m.nodes[n as usize].pts.to_sorted();
        if ts.is_empty()
            || !ts
                .iter()
                .all(|x| g.stand_in_tas.iter().any(|&(_, s)| s == *x))
        {
            return false;
        }
    }
    true
}

/// The typed-array fields of template `tm` (DECISIONS 126,
/// `Gen::nested_fields`): key, element kind, primitives beside.
fn field_ta_contracts(g: &gen::Gen, tm: model::TmplId) -> Vec<(String, Option<u8>, u16)> {
    let mut out: Vec<(String, Option<u8>, u16)> = g
        .field_tas
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), &(e, p))| (g.m.atoms[a as usize].clone(), e, p))
        .collect();
    out.sort();
    out
}

/// The nested contracts of literal template `tm`'s fields (design §18.7,
/// `Gen::nested_fields`): per described key the layouts it holds, each
/// published in `layouts_out` with its own nested contracts.
fn nested_layouts(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, Vec<String>, bool)> {
    let mut out = Vec::new();
    let mut keys: Vec<(model::Atom, Vec<model::TmplId>)> = g
        .field_layouts
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), v)| (a, v.clone()))
        .collect();
    keys.sort_unstable();
    for (a, itms) in keys {
        let mut ds = Vec::new();
        for itm in itms {
            let display = g.m.tmpls[itm as usize].display.clone();
            if !layouts_out.iter().any(|c| c.display == display) {
                let facts = templates.iter().find(|x| x.id == itm).cloned();
                // `Gen::nested_ok` admits only literal layouts of this unit.
                let (name, class_id) = match g.m.tmpls[itm as usize].kind.clone() {
                    model::TmplKind::Class { name, class_id, .. } => (name, class_id),
                    _ => (display.clone(), 0),
                };
                let facts = facts.expect("a nested layout has template facts");
                layouts_out.push(units::ClassContract {
                    display: display.clone(),
                    name,
                    class_id,
                    literal_shape: true,
                    facts,
                    methods: Vec::new(),
                    native_parent: None,
                    parent: foreign_parent(&g, itm),
                    field_fns: Vec::new(),
                    field_layouts: Vec::new(),
                    field_values: Vec::new(),
                    field_tas: Vec::new(),
                    open_keys: open_tm.contains(&itm),
                    statics: Vec::new(),
                    statics_open: false,
                    static_fns: Vec::new(),
                    static_values: Vec::new(),
                    closed: None,
                    forwards: false,
                    method_fns: Vec::new(),
                    ctor_fn: None,
                    // The shape's own birth keys: a reader's stand-in judges
                    // birth fields and the CommonJS record marker on them
                    // (DECISIONS 101).
                    birth_keys: atoms_of(g, &g.m.tmpls[itm as usize].birth_keys),
                    definite: atoms_of(g, &g.m.tmpls[itm as usize].definite),
                    ctor_leaks: true,
                    binding_prim: 0,
                });
                let nl = nested_layouts(g, templates, itm, layouts_out, open_tm);
                let fv = field_value_contracts(g, templates, itm, layouts_out, open_tm);
                if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                    c.field_layouts = nl;
                    c.field_values = fv;
                    c.field_tas = field_ta_contracts(g, itm);
                }
            }
            ds.push(display);
        }
        out.push((
            g.m.atoms[a as usize].clone(),
            ds,
            g.field_prims.contains(&(tm, a)),
        ));
    }
    out.sort();
    out
}

/// The function contract of contract function `f` after the solve (design
/// §11.2): per parameter what the callee does with its argument (the
/// markers' uses), the layouts it returns (added to `layouts_out`), the
/// result transfers.
/// A contract function's name and, when it is closed, its closed key. A
/// closed local (decision 23) is named by its function id too: locals of one
/// name (three `loader`s in moduleNameResolver.ts) are different functions,
/// with their own census entry and stand-in.
fn fn_contract_name(g: &gen::Gen, f: model::FnIdx) -> (String, Option<String>) {
    let fi = &g.m.funcs[f as usize];
    let fid = fi.func_id;
    let closed = g.view.and_then(|v| {
        v.closed
            .iter()
            .find(|&(_, &id)| id == fid)
            .map(|(nm, _)| nm.clone())
    });
    let name = match &closed {
        Some(k) if k.contains('#') && !fi.name.ends_with(&format!("#{fid}")) => {
            format!("{}#{fid}", fi.name)
        }
        _ => fi.name.clone(),
    };
    (name, closed)
}

/// A class up whose chain an `extends` value holds no class yet (an empty
/// parent cell in an early round): bottom, no instance of it exists yet,
/// so no contract describes it this round (DECISIONS 105). Published as
/// it stands, a reader would judge its instances by a chain without the
/// parent and break it for good; its unit publishes it once the value
/// arrives, and the layouts its readers read re-solve them (DECISIONS 103).
fn chain_unsettled(g: &gen::Gen, tm: model::TmplId) -> bool {
    let mut cur = Some(tm);
    while let Some(c) = cur {
        let t = &g.m.tmpls[c as usize];
        if g.m.foreign.contains_key(&c) {
            return false;
        }
        if let (None, Some((pv, _))) = (t.parent, t.parent_value) {
            let n = &g.m.nodes[pv as usize];
            // `extends` of a primitive other than null throws when the
            // class is defined: no instance with that parent exists either.
            if n.pts.is_empty() && n.prim & model::prim::NULL == 0 {
                return true;
            }
        }
        cur = t.parent;
    }
    false
}

/// The first class up `tm`'s chain that is another unit's (a stand-in),
/// by display (DECISIONS 104): the chain below it is this unit's own.
fn foreign_parent(g: &gen::Gen, tm: model::TmplId) -> Option<String> {
    let mut cur = g.m.tmpls[tm as usize].parent;
    while let Some(c) = cur {
        if g.m.foreign.contains_key(&c) {
            return Some(g.m.tmpls[c as usize].display.clone());
        }
        cur = g.m.tmpls[c as usize].parent;
    }
    None
}

fn atoms_of(g: &gen::Gen, ks: &[model::Atom]) -> Vec<String> {
    ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect()
}

fn fn_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    f: model::FnIdx,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::FnContract {
    let fi = &g.m.funcs[f as usize];
    // Its parameters carry markers (the contract states what the body does
    // with them); a contract function without them would claim it uses
    // nothing it is given.
    // A hidden capture parameter is no argument (DECISIONS 117): no marker.
    debug_assert!(
        (0..fi.params.len() as u32)
            .all(|i| g.m.marker_of.contains_key(&(f, i))
                || g.m.capture_param(f, i as usize).is_some()),
        "contract function {} has parameters without markers",
        fi.name
    );
    let uses = |reach: bool| -> Vec<u8> {
        (0..fi.params.len() as u32)
            .map(|i| {
                g.m.param_marker
                    .iter()
                    .filter(|&(t, &k)| k == (f, i) && g.m.reach_marker.contains(t) == reach)
                    .map(|(t, _)| g.m.marker_use.get(t).copied().unwrap_or(0))
                    .fold(0, |a, b| a | b)
            })
            .collect()
    };
    // A closed function's parameter every caller passes a described value
    // (decision 17): each is a stand-in here, its census (reports, store
    // effects) covers what this body does to it; the marker stands for
    // nothing, so no direct escape is published. What it reaches keeps
    // its uses. DECISIONS 148: a published method's parameter the running
    // units describe (`Model::method_params`) the same way.
    let described = g.m.closed_described.get(&f).or_else(|| {
        g.m.method_params
            .contains_key(&f)
            .then(|| g.m.method_described.get(&f))
            .flatten()
    });
    let params: Vec<u8> = uses(false)
        .into_iter()
        .enumerate()
        .map(|(i, u)| match described.and_then(|d| d.get(i)) {
            Some(true) => u & !model::MARK_ESCAPE,
            _ => u,
        })
        .collect();
    // What each parameter reaches, by the key it was read under (§18.4).
    let marker_use = |t: model::Tok| g.m.marker_use.get(&t).copied().unwrap_or(0);
    let reach: Vec<u8> = (0..fi.params.len() as u32)
        .map(|i| {
            let Some(&mk) = g.m.marker_of.get(&(f, i)) else {
                return 0;
            };
            g.m.marker_reach.get(&mk).map_or(0, |&r| marker_use(r))
        })
        .collect();
    let reach_keys: Vec<Vec<(String, u8)>> = (0..fi.params.len() as u32)
        .map(|i| {
            let Some(&mk) = g.m.marker_of.get(&(f, i)) else {
                return Vec::new();
            };
            let mut v: Vec<(model::Atom, u8)> =
                g.m.keyed_uses.get(&mk).cloned().unwrap_or_default();
            for &(a, r) in g.m.keyed_reach.get(&mk).into_iter().flatten() {
                match v.iter_mut().find(|(k, _)| *k == a) {
                    Some((_, u)) => *u |= marker_use(r),
                    None => v.push((a, marker_use(r))),
                }
            }
            // DECISIONS 126: a closed parameter every caller passes a
            // described value (decision 17): under a key where each such
            // value holds only stand-in typed arrays, the body reaches
            // nothing else there, and a typed array is a leaf whose census
            // (`Gen::reports`) covers every use here: no escape.
            let held = g.m.closed_params.get(&f).and_then(|ns| ns.get(i as usize));
            if let (Some(true), Some(&held)) = (
                g.m.closed_described
                    .get(&f)
                    .and_then(|d| d.get(i as usize))
                    .copied(),
                held,
            ) {
                for (a, u) in v.iter_mut() {
                    if described_ta_key(g, held, *a) {
                        *u &= !model::MARK_ESCAPE;
                    }
                }
            }
            let mut v: Vec<(String, u8)> = v
                .into_iter()
                .filter(|&(_, u)| u != 0)
                .map(|(a, u)| (g.m.atoms[a as usize].clone(), u))
                .collect();
            v.sort();
            v
        })
        .collect();
    let rets: Vec<model::NodeId> =
        g.m.contract_rets
            .iter()
            .filter(|(g2, _)| *g2 == f)
            .map(|&(_, n)| n)
            .collect();
    let d = describe_nodes(
        g,
        templates,
        f,
        rets,
        g.ret_top.contains(&f),
        layouts_out,
        open_tm,
    );
    // DECISIONS 140: the calls the body makes through a parameter marker,
    // each argument described as the result is.
    let mut param_calls: Vec<units::ParamCall> = Vec::new();
    for &(f2, i, key, call) in &g.m.marker_calls {
        if f2 != f {
            continue;
        }
        let args: Vec<units::Described> = g.m.calls[call as usize]
            .args
            .iter()
            .map(|&a| {
                let top = g.call_arg_top.contains(&a);
                describe_nodes(g, templates, f, vec![a], top, layouts_out, open_tm)
            })
            .collect();
        let key = key.map(|a| g.m.atoms[a as usize].clone());
        units::join_param_calls(
            &mut param_calls,
            &[units::ParamCall {
                param: i,
                key,
                args,
            }],
        );
    }
    // DECISIONS 152: the parameters' markers handed to a closed class's
    // constructor (`Model::handed`): the callers' units pass what they hold.
    let mut param_handovers: Vec<units::HandOverArg> = Vec::new();
    for &(t, ct, arg) in &g.m.handed {
        let Some((class, _, _)) = g.closed_ctors.iter().find(|c| c.2 == ct) else {
            continue;
        };
        for m in g.m.marker_members(t) {
            let own = g.m.param_marker.get(&m).is_some_and(|&(f2, i)| {
                f2 == f && i != u32::MAX && g.m.marker_of.get(&(f2, i)) == Some(&m)
            });
            if own {
                let param = g.m.param_marker[&m].1;
                param_handovers.push(units::HandOverArg {
                    param,
                    class: class.clone(),
                    arg,
                });
            }
        }
    }
    param_handovers.sort();
    param_handovers.dedup();
    // The receiver marker of a published method (DECISIONS 57); without
    // one nothing is known of what the body does to its receiver.
    let sealed = g.m.sealed_method(f);
    let (this_use, this_reach) = match g.m.marker_of.get(&(f, u32::MAX)) {
        // DECISIONS 116: a sealed class's method runs on this unit's own
        // instances only; what it does to them is in their facts.
        _ if sealed => (0, 0),
        Some(&t) => {
            let u = |t: model::Tok| g.m.marker_use.get(&t).copied().unwrap_or(0);
            (
                u(t),
                g.m.marker_reach
                    .get(&t)
                    .map_or(model::MARK_ESCAPE, |&r| u(r)),
            )
        }
        None => (model::MARK_ESCAPE, model::MARK_ESCAPE),
    };
    let this_stores = (sealed || !g.m.this_open.contains(&f)).then(|| {
        let mut ks: Vec<String> =
            g.m.this_stores
                .get(&f)
                .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
                .unwrap_or_default();
        ks.sort();
        ks
    });
    let param_stores: Vec<Option<Vec<String>>> = (0..fi.params.len() as u32)
        .map(|i| {
            (!g.m.param_open.contains(&(f, i))).then(|| {
                let mut ks: Vec<String> =
                    g.m.param_stores
                        .get(&(f, i))
                        .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
                        .unwrap_or_default();
                ks.sort();
                ks
            })
        })
        .collect();
    let mut param_flows: Vec<units::StoreFlow> =
        g.m.store_flows
            .iter()
            .filter(|((h, ..), _)| *h == f)
            .map(|(&(_, dst, reach, via, a), fl)| {
                // DECISIONS 176: what it stores of this unit; ⊤ once no
                // contract describes it.
                let value = match fl.vals {
                    Some(n) if !fl.vals_top => Some(describe_nodes(
                        g,
                        templates,
                        f,
                        vec![n],
                        false,
                        layouts_out,
                        open_tm,
                    )),
                    _ => None,
                };
                let mut from = fl.from.clone();
                from.sort();
                let mut from_reach: Vec<(u32, Option<String>)> = fl
                    .from_reach
                    .iter()
                    .map(|&(j, v)| (j, v.map(|v| g.m.atoms[v as usize].clone())))
                    .collect();
                from_reach.sort();
                units::StoreFlow {
                    dst,
                    reach,
                    via: via.map(|v| g.m.atoms[v as usize].clone()),
                    key: match a {
                        model::Key::Named(a) => Some(g.m.atoms[a as usize].clone()),
                        model::Key::Any => None,
                    },
                    from,
                    from_reach,
                    top: fl.top || (fl.vals.is_some() && fl.vals_top),
                    prim: fl.prim,
                    value,
                }
            })
            .collect();
    param_flows
        .sort_by(|a, b| (a.dst, a.reach, &a.via, &a.key).cmp(&(b.dst, b.reach, &b.via, &b.key)));
    let mut this_calls: Vec<String> =
        g.m.this_calls
            .get(&f)
            .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
            .unwrap_or_default();
    this_calls.sort();
    let (name, closed) = fn_contract_name(g, f);
    // DECISIONS 162: the array cells it stores an argument into.
    let param_cells: Vec<(u32, String)> = g
        .cell_flows
        .iter()
        .filter(|(g_f, _, _)| *g_f == f)
        .map(|(_, i, k)| (*i, k.clone()))
        .collect();
    units::FnContract {
        method: g.m.published_this.contains_key(&f),
        this_use,
        this_reach,
        this_stores,
        this_calls,
        param_stores,
        param_flows,
        param_calls,
        param_handovers,
        param_cells,
        name,
        closed,
        params,
        reach,
        reach_keys,
        // A rest parameter takes only the arguments past the others
        // (design §18.11); `arguments` aliases every argument.
        variadic: fi.has_arguments,
        rest: fi.rest.is_some() && !fi.has_arguments,
        ret: d.layouts,
        ret_params: d.params,
        ret_reach: d.reach,
        ret_reach_keys: d.reach_keys,
        ret_unknown: d.unknown,
        ret_value: d.value,
        ret_prim: d.prim,
        lexical_this: fi.lexical_this || !reads_this(g, f),
    }
}

/// DECISIONS 147: function `f` of this unit named in a value it hands out.
/// The reader takes the owner's entry by the name (`Gen::owner_fn`,
/// DECISIONS 106); until the owner published one, this entry claims
/// nothing: every argument escapes, the result is unknown.
/// DECISIONS 165: this unit's published methods another unit cannot name:
/// a prototype key holding more than one function (the class contract
/// lists a method only alone under its key, `ClassContract::methods`) or a
/// computed member. A reader calls those through ⊤, which publishes no
/// run, so the owner publishes them as run itself: none reads its
/// parameter markers as having nothing beside them.
fn unnamed_methods(m: &model::Model) -> impl Iterator<Item = model::FnIdx> + '_ {
    let proto = |h: model::Tok| matches!(m.toks[h as usize], model::TokKind::Proto(_));
    let fns = move |n: model::NodeId| {
        m.nodes[n as usize]
            .pts
            .to_sorted()
            .into_iter()
            .map(|t| m.tok_fn(t))
            .filter(|&f| f != model::NO_FN && m.method_described.contains_key(&f))
    };
    m.fields
        .iter()
        .filter(move |(&(h, _), &n)| proto(h) && m.nodes[n as usize].pts.to_sorted().len() > 1)
        .flat_map(move |(_, &n)| fns(n))
        .chain(
            m.field_any
                .iter()
                .filter(move |(&h, _)| proto(h))
                .flat_map(move |(_, &n)| fns(n)),
        )
}

fn fn_named(g: &gen::Gen, f: model::FnIdx) -> units::FnContract {
    let fi = &g.m.funcs[f as usize];
    let n = fi.params.len();
    let (name, closed) = fn_contract_name(g, f);
    units::FnContract {
        method: g.m.published_this.contains_key(&f),
        name,
        closed,
        params: vec![model::MARK_ESCAPE; n],
        reach: vec![model::MARK_ESCAPE; n],
        reach_keys: vec![Vec::new(); n],
        variadic: fi.has_arguments,
        rest: fi.rest.is_some() && !fi.has_arguments,
        ret: None,
        ret_params: Vec::new(),
        ret_reach: Vec::new(),
        ret_reach_keys: Vec::new(),
        ret_unknown: true,
        ret_value: Default::default(),
        ret_prim: model::prim::ALL,
        lexical_this: fi.lexical_this,
        this_use: model::MARK_ESCAPE,
        this_reach: model::MARK_ESCAPE,
        this_stores: None,
        this_calls: Vec::new(),
        param_stores: vec![None; n],
        param_flows: Vec::new(),
        param_calls: Vec::new(),
        param_handovers: Vec::new(),
        param_cells: Vec::new(),
    }
}

/// A reach marker reaches itself under every key (§18.4): what it stands
/// for in the result is its reach through the keys the body read on it,
/// repeatedly; a computed-key read (its all-fields node feeds a reader)
/// is any key (DECISIONS 138).
fn keys_read(g: &gen::Gen, t: model::Tok) -> Option<Vec<String>> {
    if g.m
        .all_fields
        .get(&t)
        .is_some_and(|&n| !g.m.nodes[n as usize].succ.is_empty())
    {
        return None;
    }
    let mut v: Vec<String> = g.m.field_keys.get(&t).map_or_else(Vec::new, |ks| {
        ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect()
    });
    v.sort();
    v.dedup();
    Some(v)
}

/// DECISIONS 158: a parameter's path marker stands for the values at
/// its path, each step with the keys its folds repeat there.
fn path_loops(g: &gen::Gen, t: model::Tok) -> Option<Vec<String>> {
    let l = g.m.path_loops.get(&t).map_or(&[][..], |l| &l[..]);
    let mut v = Vec::new();
    for k in l {
        v.push(g.m.atoms[(*k)? as usize].clone());
    }
    v.sort();
    v.dedup();
    Some(v)
}

/// The path of `f`'s parameter path marker `t`, as the reader walks it.
fn reach_key_of(g: &gen::Gen, f: model::FnIdx, t: model::Tok) -> Option<units::ReachKey> {
    if !g.m.is_path_marker(t) {
        return None;
    }
    let ks = g.m.path_keys(t);
    let (r, a0) = ks[0];
    let (p, _) = g.m.path_parent[&r];
    let &(_, i) = g.m.param_marker.get(&p)?;
    if g.m.marker_of.get(&(f, i)) != Some(&p) {
        return None;
    }
    let steps = ks[1..]
        .iter()
        .map(|&(x, k)| {
            (
                k.map(|a| g.m.atoms[a as usize].to_string()),
                path_loops(g, x),
            )
        })
        .collect();
    Some((i, g.m.atoms[a0? as usize].clone(), path_loops(g, r), steps))
}

/// What a value contract function `f` hands the caller's unit may be
/// (DECISIONS 140): its result, or an argument of a call through one of its
/// parameters; `nodes` hold it, `top` when no contract describes it.
fn describe_nodes(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    f: model::FnIdx,
    nodes: Vec<model::NodeId>,
    top: bool,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::Described {
    let mut ret_params: Vec<u32> = Vec::new();
    let mut ret_reach: Vec<u32> = Vec::new();
    let mut ret_reach_keys: Vec<units::ReachKey> = Vec::new();
    let keys_read = |t: model::Tok| keys_read(g, t);
    let join_keys = |a: Option<Vec<String>>, b: Option<Vec<String>>| -> Option<Vec<String>> {
        let mut v = a?;
        v.extend(b?);
        v.sort();
        v.dedup();
        Some(v)
    };
    let path_of = |t: model::Tok| reach_key_of(g, f, t);
    let mut ret_unknown = false;
    let mut ret_value = units::ValueContract::default();
    let mut ret_prim = nodes.iter().fold(0, |p, &n| p | g.m.nodes[n as usize].prim);
    let layouts = (!top && !nodes.is_empty()).then(|| {
        let mut ds: Vec<String> = Vec::new();
        for n in nodes {
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let mut toks: Vec<model::Tok> = Vec::new();
            for t in g.m.nodes[n as usize].pts.to_sorted() {
                // Its keyed reach is what each member reaches under the key,
                // then under the keys read on it: one named key of this
                // function's own parameter, or a deeper reach of it (a
                // keyed reach marker: its key, then the keys read on both,
                // DECISIONS 138; the any-key reach: what the parameter
                // reaches). Anything else (an escaped member, another
                // function's) reads as ⊤, another unit's value whole
                // (DECISIONS 119: no ⊤ beside the marker stands for it).
                if let Some(&(p, a)) = g.m.keyed_parent.get(&t) {
                    let ms = g.m.marker_members(p);
                    let a = g.m.atoms[a as usize].clone();
                    for &m in &ms {
                        // A member on a path of this function's own
                        // parameter: that path, then `a` (DECISIONS 158).
                        if let Some(mut k) = path_of(m).filter(|_| !g.m.marker_is_top(m)) {
                            k.3.push((Some(a.clone()), keys_read(t)));
                            if !ret_reach_keys.contains(&k) {
                                ret_reach_keys.push(k);
                            }
                            continue;
                        }
                        let own = match g.m.param_marker.get(&m) {
                            Some(&(f2, i)) if f2 == f && !g.m.marker_is_top(m) => {
                                g.m.marker_of.get(&(f, i)).map(|&mk| (i, mk))
                            }
                            _ => None,
                        };
                        let deeper = own.and_then(|(i, mk)| {
                            g.m.keyed_reach
                                .get(&mk)?
                                .iter()
                                .find(|&&(_, r)| r == m)
                                .map(|&(k0, _)| (i, k0))
                        });
                        match own {
                            Some((i, mk)) if mk == m => {
                                let k = (i, a.clone(), keys_read(t), Vec::new());
                                if !ret_reach_keys.contains(&k) {
                                    ret_reach_keys.push(k);
                                }
                            }
                            Some((i, _)) if deeper.is_some() => {
                                let k0 = deeper
                                    .map(|(_, k0)| g.m.atoms[k0 as usize].clone())
                                    .unwrap();
                                let ks = join_keys(
                                    join_keys(keys_read(m), keys_read(t)),
                                    Some(vec![a.clone()]),
                                );
                                let k = (i, k0, ks, Vec::new());
                                if !ret_reach_keys.contains(&k) {
                                    ret_reach_keys.push(k);
                                }
                            }
                            Some((i, mk)) if g.m.marker_reach.get(&mk) == Some(&m) => {
                                if !ret_reach.contains(&i) {
                                    ret_reach.push(i);
                                }
                            }
                            _ => {
                                ret_unknown = true;
                                ret_prim |= model::prim::ALL;
                            }
                        }
                    }
                    if ms.is_empty() {
                        ret_unknown = true;
                        ret_prim |= model::prim::ALL;
                    }
                    continue;
                }
                toks.extend(g.m.marker_members(t));
            }
            for t in toks {
                // An escaped marker reads as ⊤ (DECISIONS 42).
                if matches!(
                    g.m.toks[t as usize],
                    model::TokKind::Top | model::TokKind::BuiltinFn
                ) || g.m.marker_is_top(t)
                {
                    ret_unknown = true;
                    // An escaped marker is another unit's value whole
                    // (DECISIONS 119): any primitive too.
                    if g.m.is_marker(t) {
                        ret_prim |= model::prim::ALL;
                    }
                    continue;
                }
                // A value no contract describes escaped alone
                // (`Gen::publish_result`, DECISIONS 175): unknown.
                if !g.result_tok_ok(f, t) {
                    ret_unknown = true;
                    continue;
                }
                // A builtin, an array (DECISIONS 94), a primitive method's
                // container (184).
                let extra = match g.m.toks[t as usize] {
                    model::TokKind::Builtin(_) => true,
                    model::TokKind::Obj(..) => g.m.is_cell_arr(t),
                    _ => false,
                };
                if extra {
                    let vc =
                        value_contract_of_toks(g, templates, 0, vec![t], &[], layouts_out, open_tm);
                    ret_value.join(&vc);
                    continue;
                }
                // DECISIONS 147: a function it hands out, by name: the reader
                // calls it through its owner's entry (DECISIONS 106), a
                // function of this unit is one (`Gen::publish_result` made it
                // a contract function, `own_fns_out`); another unit's by its
                // stand-in contract.
                if let model::TokKind::Func(f2, _) = g.m.toks[t as usize] {
                    let fc = match g.stand_in_fc.get(&t) {
                        Some(&fc) => fc.clone(),
                        None if g.m.funcs[f2 as usize].func_id != u32::MAX => fn_named(g, f2),
                        None => {
                            ret_unknown = true;
                            continue;
                        }
                    };
                    if !ret_value.fns.iter().any(|c| c.name == fc.name) {
                        ret_value.fns.push(fc);
                    }
                    continue;
                }
                if let Some(k) = path_of(t) {
                    if !ret_reach_keys.contains(&k) {
                        ret_reach_keys.push(k);
                    }
                    continue;
                }
                if let Some(&(_, i)) = g.m.param_marker.get(&t) {
                    // What the parameter reaches through one named key: a
                    // keyed reach marker of the parameter's own marker.
                    let keyed =
                        g.m.marker_of
                            .get(&(f, i))
                            .and_then(|mk| g.m.keyed_reach.get(mk)?.iter().find(|&&(_, r)| r == t));
                    if let Some(&(a, _)) = keyed {
                        let k = (i, g.m.atoms[a as usize].clone(), keys_read(t), Vec::new());
                        if !ret_reach_keys.contains(&k) {
                            ret_reach_keys.push(k);
                        }
                        continue;
                    }
                    let v = if g.m.reach_marker.contains(&t) {
                        &mut ret_reach
                    } else {
                        &mut ret_params
                    };
                    if !v.contains(&i) {
                        v.push(i);
                    }
                    continue;
                }
                let Some(tm) = g.m.tmpl_of(t) else { continue };
                let display = g.m.tmpls[tm as usize].display.clone();
                if ds.contains(&display) {
                    continue;
                }
                // Another unit's layout (a stand-in: `return defaults`) is
                // forwarded with that unit's contract, its proof; a reader's
                // census report breaks it at its owner (§11.3).
                if let Some(cc) = g.stand_in_cc.get(&tm) {
                    if !layouts_out.iter().any(|c| c.display == display) {
                        layouts_out.push(cc.clone());
                    }
                    ds.push(display);
                    continue;
                }
                if let (
                    Some(facts),
                    model::TmplKind::Class {
                        name,
                        class_id,
                        literal_shape,
                        ..
                    },
                ) = (
                    templates.iter().find(|x| x.id == tm),
                    g.m.tmpls[tm as usize].kind.clone(),
                ) {
                    if !layouts_out.iter().any(|c| c.display == display) && !chain_unsettled(&g, tm)
                    {
                        layouts_out.push(units::ClassContract {
                            display: display.clone(),
                            name,
                            class_id,
                            literal_shape,
                            facts: facts.clone(),
                            methods: Vec::new(),
                            native_parent: None,
                            parent: foreign_parent(&g, tm),
                            field_fns: Vec::new(),
                            field_layouts: Vec::new(),
                            field_values: Vec::new(),
                            field_tas: Vec::new(),
                            open_keys: open_tm.contains(&tm),
                            statics: Vec::new(),
                            statics_open: false,
                            static_fns: Vec::new(),
                            static_values: Vec::new(),
                            closed: None,
                            forwards: false,
                            method_fns: Vec::new(),
                            ctor_fn: None,
                            // The shape's own birth keys: a reader's stand-in judges
                            // birth fields and the CommonJS record marker on them
                            // (DECISIONS 101).
                            birth_keys: atoms_of(g, &g.m.tmpls[tm as usize].birth_keys),
                            definite: atoms_of(g, &g.m.tmpls[tm as usize].definite),
                            ctor_leaks: true,
                            binding_prim: 0,
                        });
                        let nl = nested_layouts(g, templates, tm, layouts_out, open_tm);
                        let fv = field_value_contracts(g, templates, tm, layouts_out, open_tm);
                        if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                            c.field_layouts = nl;
                            c.field_values = fv;
                            c.field_tas = field_ta_contracts(g, tm);
                        }
                        // Its field functions, as a value contract's (DECISIONS 78,
                        // `Gen::note_field_fns`). The layout is out first, so a field
                        // function returning this literal again does not recurse.
                        if literal_shape {
                            let nt = g.m.nodes[n as usize].pts.to_sorted();
                            let ff = literal_field_fns(g, templates, &nt, tm, layouts_out, open_tm);
                            if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                                c.field_fns = ff;
                            }
                        }
                    }
                }
                ds.push(display);
            }
        }
        ds.sort();
        ds
    });
    ret_params.sort();
    ret_reach.sort();
    ret_reach_keys.sort();
    units::Described {
        layouts,
        params: ret_params,
        reach: ret_reach,
        reach_keys: ret_reach_keys,
        unknown: ret_unknown,
        value: ret_value,
        prim: ret_prim,
    }
}

/// Function `f`'s body may read its receiver: `this` or `super` in it (or
/// in a nested arrow), or a direct eval may (design §18.2: `ns.f()` passes
/// the namespace as the receiver of a function that never reads it). A
/// body not found among the module's functions may.
fn reads_this(g: &gen::Gen, f: model::FnIdx) -> bool {
    let fi = &g.m.funcs[f as usize];
    if g.unit_in.is_some_and(|u| u.eval_scopes()) {
        return true;
    }
    let Some((_, module)) = g.modules.get(fi.module as usize) else {
        return true;
    };
    match module.functions.iter().find(|h| h.id == fi.func_id) {
        Some(h) => h.body.iter().any(gen_call::stmt_reads_this),
        None => true,
    }
}

/// What this unit's calls pass to closed function `nm` of another unit
/// (decision 13): per parameter of its stand-in, the value contract of the
/// arguments: the layouts of literal-shape objects of this unit, of
/// contract-class instances and of other units' stand-ins (forwarded), an
/// unknown value for anything else (a caller's own parameter marker
/// included: its callers pass anything, unless the caller is closed too). `top` when the import holds
/// anything but stand-in functions (⊤, a broken contract): its calls may
/// pass anything to the function.
/// A published class's closed key (design §18.13): its module path, name
/// and constructor parameter count, when the class is closed in its unit
/// (`UnitView::closed` holds its constructor) and the constructor reads no
/// `arguments` and has no rest parameter (a construction's extra arguments
/// reach nothing).
fn closed_of(
    g: &gen::Gen,
    path: &str,
    mi: u32,
    tm: model::TmplId,
) -> Option<(String, String, u32)> {
    let f = g.m.tmpls[tm as usize].ctor?;
    let fi = &g.m.funcs[f as usize];
    let model::TmplKind::Class { name, .. } = &g.m.tmpls[tm as usize].kind else {
        return None;
    };
    let closed = g.view?.closed.get(name) == Some(&fi.func_id);
    (mi == 0 && closed && !fi.has_arguments && fi.rest.is_none())
        .then(|| (path.to_string(), name.clone(), fi.params.len() as u32))
}

/// DECISIONS 157: a class of this unit that is published
/// (`UnitView::published`): its holders forward its stand-in.
fn forwards_of(g: &gen::Gen, mi: u32, tm: model::TmplId) -> bool {
    let model::TmplKind::Class { name, .. } = &g.m.tmpls[tm as usize].kind else {
        return false;
    };
    mi == 0 && g.view.is_some_and(|v| v.published.contains(name))
}

/// The value contract of what a node may hold (design §12): the layouts and
/// classes of contracts, this unit's contract functions, other units'
/// stand-ins, builtins, primitive kinds; anything else is unknown.
fn value_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    n: model::NodeId,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ValueContract {
    let node = &g.m.nodes[n as usize];
    value_contract_of_toks(
        g,
        templates,
        node.prim,
        node.pts.to_sorted(),
        contracts_out,
        layouts_out,
        open_tm,
    )
}

/// `value_contract_of` over these values.
fn value_contract_of_toks(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    prim: u16,
    toks: Vec<model::Tok>,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ValueContract {
    let mut vc = units::ValueContract {
        prim,
        ..Default::default()
    };
    for &t in &toks {
        let (tm, class) = match g.m.toks[t as usize] {
            model::TokKind::Top | model::TokKind::BuiltinFn => {
                vc.unknown = true;
                continue;
            }
            model::TokKind::Builtin(ns) => {
                if !vc.builtins.contains(&ns) {
                    vc.builtins.push(ns);
                }
                continue;
            }
            // DECISIONS 137: a marker with values beside it (DECISIONS 74)
            // is described by them; it adds no kind of its own.
            model::TokKind::Obj(..) if g.m.is_marker(t) && g.m.beside_values(t) => continue,
            // An array (DECISIONS 94): its elements are cell `arr:<display>`.
            model::TokKind::Obj(..) if g.m.is_cell_arr(t) => {
                let d = g.arr_name(t);
                let v = if g.m.is_box(t) {
                    &mut vc.boxes
                } else {
                    &mut vc.arrays
                };
                if !v.contains(&d) {
                    v.push(d);
                    v.sort();
                }
                continue;
            }
            model::TokKind::Obj(tm, _) => (tm, false),
            model::TokKind::ClassObj(tm) => (tm, true),
            // A function of this unit that escaped before any contract
            // named it is no contract function: it left alone, and the
            // value says unknown beside the rest (DECISIONS 174, 178).
            model::TokKind::Func(f, _)
                if g.m.funcs[f as usize].func_id != u32::MAX && !g.m.contract_fns.contains(&f) =>
            {
                vc.unknown = true;
                continue;
            }
            model::TokKind::Func(f, _) if g.m.funcs[f as usize].func_id != u32::MAX => {
                // A contract function of this unit (`Gen::contract_check`).
                let fc = fn_contract_of(g, templates, f, layouts_out, open_tm);
                if !vc.fns.iter().any(|c| c.name == fc.name) {
                    vc.fns.push(fc);
                }
                continue;
            }
            _ => {
                if let Some(&fc) = g.stand_in_fc.get(&t) {
                    if !vc.fns.iter().any(|c| c.name == fc.name) {
                        vc.fns.push(fc.clone());
                    }
                }
                continue;
            }
        };
        let display = &g.m.tmpls[tm as usize].display;
        let cc = g
            .stand_in_cc
            .get(&tm)
            .cloned()
            .or_else(|| {
                contracts_out
                    .iter()
                    .find(|(_, c)| &c.display == display)
                    .map(|(_, c)| c.clone())
            })
            .or_else(|| {
                let facts = templates.iter().find(|x| x.id == tm)?.clone();
                if chain_unsettled(&g, tm) {
                    return None;
                }
                let (name, class_id, literal_shape) = match g.m.tmpls[tm as usize].kind.clone() {
                    model::TmplKind::Class {
                        name,
                        class_id,
                        literal_shape,
                        ..
                    } => (name, class_id, literal_shape),
                    // An enum's object (§18.2): a literal layout with
                    // no class (nothing guards it, so no class id).
                    // A plain object literal (DECISIONS 94) the same way.
                    model::TmplKind::Lit { .. } => (display.clone(), 0, true),
                    _ => return None,
                };
                Some(units::ClassContract {
                    display: display.clone(),
                    name,
                    class_id,
                    literal_shape,
                    facts,
                    methods: Vec::new(),
                    native_parent: None,
                    parent: foreign_parent(&g, tm),
                    field_fns: Vec::new(),
                    field_layouts: Vec::new(),
                    field_values: Vec::new(),
                    field_tas: Vec::new(),
                    open_keys: open_tm.contains(&tm),
                    statics: Vec::new(),
                    statics_open: false,
                    static_fns: Vec::new(),
                    static_values: Vec::new(),
                    closed: None,
                    forwards: false,
                    method_fns: Vec::new(),
                    ctor_fn: None,
                    // The shape's own birth keys: a reader's stand-in judges
                    // birth fields and the CommonJS record marker on them
                    // (DECISIONS 101).
                    birth_keys: atoms_of(g, &g.m.tmpls[tm as usize].birth_keys),
                    definite: atoms_of(g, &g.m.tmpls[tm as usize].definite),
                    ctor_leaks: true,
                    binding_prim: 0,
                })
            });
        let Some(mut cc) = cc else {
            vc.unknown = true;
            continue;
        };
        if !class
            && !g.stand_in_cc.contains_key(&tm)
            && !vc.layouts.iter().any(|c| c.display == cc.display)
        {
            // DECISIONS 153: a layout whose fields hold it again (a list's
            // or a tree's link beside forwarded stand-ins) is described once.
            // While its fields are described it stands in `layouts_out`, so
            // the description meeting it again takes that copy instead of
            // recursing; the reader builds the outer one first (its fields
            // hold the inner one) and finds it again by display
            // (`Gen::stand_in_class`). It leaves `layouts_out` afterwards: a
            // value contract's layout is not published. A published layout
            // is the reader's in any case (`Gen::other_layout`, DECISIONS 106).
            match layouts_out.iter().find(|c| c.display == cc.display) {
                Some(p) => cc = p.clone(),
                None => {
                    layouts_out.push(cc.clone());
                    cc.field_fns =
                        literal_field_fns(&g, templates, &toks, tm, layouts_out, open_tm);
                    cc.field_layouts = nested_layouts(g, templates, tm, layouts_out, open_tm);
                    cc.field_values = field_value_contracts(g, templates, tm, layouts_out, open_tm);
                    cc.field_tas = field_ta_contracts(g, tm);
                    if let Some(i) = layouts_out.iter().position(|c| c.display == cc.display) {
                        layouts_out.remove(i);
                    }
                }
            }
        }
        let v = if class {
            &mut vc.classes
        } else {
            &mut vc.layouts
        };
        if !v.iter().any(|c| c.display == cc.display) {
            v.push(cc);
        }
    }
    vc
}

/// DECISIONS 152: `t` is what an argument marker reaches (its reach, or
/// its reach under a key, transitively), and a member of that argument
/// marker stands for values no contract describes: a parameter marker that
/// is no closed caller's (decision 13: its callers' values are beside it)
/// and has no values beside it (DECISIONS 142).
fn arg_reach_unknown(g: &gen::Gen, t: model::Tok) -> bool {
    if !g.m.is_arg_marker(t) || !g.m.reach_marker.contains(&t) {
        return false;
    }
    let mut a = t;
    while g.m.reach_marker.contains(&a) {
        let up =
            g.m.marker_reach
                .iter()
                .find(|(_, &r)| r == a)
                .map(|(&p, _)| p)
                .or_else(|| {
                    g.m.keyed_reach
                        .iter()
                        .find(|(_, v)| v.iter().any(|&(_, r)| r == a))
                        .map(|(&p, _)| p)
                });
        match up {
            Some(p) => a = p,
            // No marker it is the reach of: nothing says what it holds.
            None => return true,
        }
    }
    g.m.marker_members(a).into_iter().any(|m| {
        !g.m.param_marker
            .get(&m)
            .is_some_and(|(f, _)| g.m.closed_params.contains_key(f))
            && !g.m.beside_values(m)
    })
}

fn arg_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    toks: Vec<model::Tok>,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ArgContract {
    let mut ac = units::ArgContract::default();
    if toks.is_empty()
        || !toks
            .iter()
            .all(|t| g.stand_in_fc.contains_key(t) || g.closed_ctors.iter().any(|c| c.2 == *t))
    {
        ac.top = true;
        return ac;
    }
    for t in toks {
        let fi = &g.m.funcs[g.m.tok_fn(t) as usize];
        // A closed class's stand-in constructor: its body reads no
        // `arguments` (`closed_of`); its parameters hand over what they
        // hold (`Gen::stand_in_class`).
        let closed_ctor = g.closed_ctors.iter().any(|c| c.2 == t);
        if fi.has_arguments && !closed_ctor {
            ac.top = true;
            continue;
        }
        if ac.params.len() < fi.params.len() {
            ac.params
                .resize(fi.params.len(), units::ValueContract::default());
        }
        for (i, &p) in fi.params.iter().enumerate() {
            let node = &g.m.nodes[p as usize];
            let vc = &mut ac.params[i];
            // A call passing fewer arguments passes undefined: every call of
            // the stand-in (direct, spread, bound, a builtin's callback)
            // binds through `bind_args`, which puts UNDEF in its missing
            // parameters, so the node's kinds already say so (DECISIONS 77).
            vc.prim |= node.prim;
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let toks = node.pts.to_sorted();
            // DECISIONS 152: what an argument marker reaches (`p.k`) has no
            // members of its own: it reads as what its members reach, a
            // value no contract describes here (as a parameter marker's
            // reach is), not as nothing.
            if toks.iter().any(|&t| arg_reach_unknown(g, t)) {
                vc.unknown = true;
                vc.prim |= model::prim::ALL;
            }
            for t in toks.into_iter().flat_map(|t| g.m.marker_members(t)) {
                // Another unit's function value (decision 19): by its
                // contract; the callee's calls of it publish what they pass
                // under its name. This unit's own function is unknown there
                // (it carries no markers unless published): passed to an
                // undescribed parameter it escapes, its parameters ⊤.
                if let model::TokKind::Func(f, _) = g.m.toks[t as usize] {
                    match g.stand_in_fc.get(&t) {
                        Some(&fc) if !vc.fns.iter().any(|c| c.name == fc.name) => {
                            vc.fns.push(fc.clone())
                        }
                        Some(_) => {}
                        // This unit's closed function published through its
                        // closed contract (decision 23): its markers carry
                        // the body's uses; the callee publishes its calls.
                        // Markers and parameters are the function's, so one
                        // published token (another context's) covers it.
                        None if g.m.closed_params.contains_key(&f)
                            && g.m.contract_fns.contains(&f)
                            && g.m.closed_pub.iter().any(|&u| g.m.tok_fn(u) == f) =>
                        {
                            let fc = fn_contract_of(g, templates, f, layouts_out, open_tm);
                            if !vc.fns.iter().any(|c| c.name == fc.name) {
                                vc.fns.push(fc);
                            }
                        }
                        None => vc.unknown = true,
                    }
                    continue;
                }
                let model::TokKind::Obj(tm, _) = g.m.toks[t as usize] else {
                    vc.unknown = true;
                    continue;
                };
                // DECISIONS 149e: an array leaves by its element cell
                // (`Gen::contract_check`), named as a value contract names it (94).
                if g.m.is_cell_arr(t) {
                    let d = g.arr_name(t);
                    let v = if g.m.is_box(t) {
                        &mut vc.boxes
                    } else {
                        &mut vc.arrays
                    };
                    if !v.contains(&d) {
                        v.push(d);
                        v.sort();
                    }
                    continue;
                }
                let display = &g.m.tmpls[tm as usize].display;
                // DECISIONS 152: a parameter marker handed to a closed
                // class's constructor: its function's contract hands it
                // over (`Model::handed`), and the callers' units publish
                // what they pass here.
                if closed_ctor
                    && g.m.param_marker.get(&t).is_some_and(|&(f, j)| {
                        j != u32::MAX && g.m.marker_of.get(&(f, j)) == Some(&t)
                    })
                {
                    continue;
                }
                // A closed caller passing its own parameter on: what its
                // callers pass is beside the marker (decision 13).
                if g.m
                    .param_marker
                    .get(&t)
                    .is_some_and(|(f, _)| g.m.closed_params.contains_key(f))
                {
                    continue;
                }
                // DECISIONS 142: a marker with values beside it (a published
                // method's receiver and what it reaches, DECISIONS 107) is
                // described by them, as a value contract's is (DECISIONS 137).
                if g.m.beside_values(t) {
                    continue;
                }
                if g.m.param_marker.contains_key(&t) || g.unit_in.is_some_and(|u| u.broken(display))
                {
                    vc.unknown = true;
                    // A marker is the whole argument (DECISIONS 119).
                    if g.m.param_marker.contains_key(&t) {
                        vc.prim |= model::prim::ALL;
                    }
                    continue;
                }
                let cc = if let Some(cc) = g.stand_in_cc.get(&tm) {
                    Some(cc.clone())
                } else if let Some((_, c)) =
                    contracts_out.iter().find(|(_, c)| &c.display == display)
                {
                    Some(c.clone())
                } else {
                    match (
                        templates.iter().find(|x| x.id == tm),
                        g.m.tmpls[tm as usize].kind.clone(),
                    ) {
                        (
                            Some(facts),
                            model::TmplKind::Class {
                                name,
                                class_id,
                                literal_shape: true,
                                ..
                            },
                        ) => Some(units::ClassContract {
                            display: display.clone(),
                            name,
                            class_id,
                            literal_shape: true,
                            facts: facts.clone(),
                            methods: Vec::new(),
                            native_parent: None,
                            parent: foreign_parent(&g, tm),
                            field_fns: Vec::new(),
                            field_layouts: Vec::new(),
                            field_values: Vec::new(),
                            field_tas: Vec::new(),
                            open_keys: open_tm.contains(&tm),
                            statics: Vec::new(),
                            statics_open: false,
                            static_fns: Vec::new(),
                            static_values: Vec::new(),
                            closed: None,
                            forwards: false,
                            method_fns: Vec::new(),
                            ctor_fn: None,
                            // The shape's own birth keys: a reader's stand-in judges
                            // birth fields and the CommonJS record marker on them
                            // (DECISIONS 101).
                            birth_keys: atoms_of(g, &g.m.tmpls[tm as usize].birth_keys),
                            definite: atoms_of(g, &g.m.tmpls[tm as usize].definite),
                            ctor_leaks: true,
                            binding_prim: 0,
                        }),
                        _ => None,
                    }
                };
                match cc {
                    Some(mut cc) if !vc.layouts.iter().any(|c| c.display == cc.display) => {
                        // Its field functions, as a value contract's (`Gen::note_field_fns`).
                        if !g.stand_in_cc.contains_key(&tm) {
                            cc.field_fns = literal_field_fns(
                                g,
                                templates,
                                &node.pts.to_sorted(),
                                tm,
                                layouts_out,
                                open_tm,
                            );
                        }
                        vc.layouts.push(cc)
                    }
                    Some(_) => {}
                    None => vc.unknown = true,
                }
            }
        }
    }
    ac
}

/// [`compute`] for one unit of a per-unit run: `published` is what the other
/// units can name of its modules (`units::published_surface`).
pub(crate) fn compute_in<'a>(
    modules: &'a [(String, &'a Module)],
    cfg: &FactsConfig,
    published: Option<&'a units::Published>,
    view: Option<&'a decl_units::UnitView>,
) -> ProgramFacts {
    let t0 = std::time::Instant::now();
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut g = gen::Gen::new(sorted.clone(), cfg.relations, cfg.explain);
    g.export_sources = cfg.export_sources.clone();
    g.unit_in = published;
    g.view = view;
    let interp = cfg.dynamic_code.clone().unwrap_or_else(|| "-".into());
    let dynamic_reach = cfg.dynamic_code_reach.globals as u8;
    g.m.rel(|| format!("dynamicreach {dynamic_reach}"));
    g.m.rel(|| format!("interp {}", interp.replace(' ', "\\s")));
    if cfg.profile {
        g.m.profile = Some(Default::default());
    }
    // The bounds are in force from the first constraint: generation already
    // solves online (`Model::cons` and `Model::edge` apply each rule to what
    // its node holds), so it is part of the solve they bound.
    if let Some(secs) = cfg.budget_secs {
        g.m.budget = Some((t0, std::time::Duration::from_secs(secs)));
    }
    g.m.max_pts = cfg.max_pts.unwrap_or(0);
    g.m.max_work = cfg.max_work.unwrap_or(0);
    g.m.max_nodes = cfg.max_pts.map(|p| (p / 8) as usize).unwrap_or(0);
    g.m.interp_free = cfg.dynamic_code.is_none()
        && !cfg.dynamic_code_reach.globals
        && cfg.dynamic_code_reach.lexical_modules.is_empty()
        && cfg.dynamic_code_reach.sites.is_empty();
    if cfg.dynamic_code.is_some() || cfg.dynamic_code_reach.globals {
        // Interpreted code reads and writes any global (E20); the
        // prototype-write guard does not cover it.
        let gt =
            g.m.tok(model::TokKind::Builtin(model::BuiltinNs::GlobalThis));
        builtins::note_native_write(&mut g.m, gt, None, "interpreted code may run");
    }
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    if cfg.unit {
        // The closed contracts first (decision 19): publishing a closed
        // function is no escape only once its contract is known.
        g.closed_params();
        g.published_receivers();
        g.published_args();
        g.hold_binding_writes();
        g.unit_boundary(published);
    }
    let gen_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if cfg.profile {
        eprintln!("perry: wpfacts after generation {}", g.m.size_line());
    }
    let t1 = std::time::Instant::now();
    g.m.log_seed_accessors();
    for s in &g.read_sites {
        let (m0, o, k, r, c) = (s.module, s.byte_offset, s.key, s.recv, s.is_call as u8);
        g.m.rel(|| format!("readsite {m0} {o} {k} {r} {c}"));
    }
    if cfg.dynamic_code.is_some() || cfg.dynamic_code_reach.globals {
        // Interpreted code can catch what compiled code throws (a compiled
        // function it calls throws into its `try`): thrown values reach the
        // summary node.
        let r = g.m.reason("thrown where interpreted code can catch it");
        let thrown = g.m.thrown;
        g.m.sink_node(thrown, r);
    }
    g.m.summarize_and_replay();
    if cfg.profile || cfg.explain {
        eprintln!("perry: wpfacts {}", g.m.sum.stats);
    }
    g.m.solve();
    let first_solve_ms = t1.elapsed().as_secs_f64() * 1000.0;
    // Past the bound the unit claims nothing (sound): its contracts are
    // neither checked nor published (113-CWL3).
    let mut checks = 0u64;
    while cfg.unit && !g.m.aborted && {
        checks += 1;
        // DECISIONS 171: a cell its solve came to consume is seeded first.
        let woke = g.wake_cells();
        g.contract_check() | woke
    } {
        g.m.solve();
    }
    let checks_ms = t1.elapsed().as_secs_f64() * 1000.0 - first_solve_ms;
    g.m.settle_keyed_uses();
    g.settle_parent_layouts();
    if cfg.profile {
        eprintln!(
            "perry: wpfacts post-solve: first solve {first_solve_ms:.0} ms, {checks} contract checks with their solves {checks_ms:.0} ms, settle {:.0} ms",
            t1.elapsed().as_secs_f64() * 1000.0 - first_solve_ms - checks_ms
        );
    }
    if cfg.profile || cfg.explain {
        let mut kinds = [0usize; 6];
        let mut fnkeyed = 0;
        for &(k, l, _) in g.m.sum.inst_key.keys() {
            kinds[k as usize] += 1;
            fnkeyed += (k == 1 && l == summary::LAY_NONE) as usize;
        }
        eprintln!(
            "perry: wpfacts instances {} (site {} layout {} of them function-keyed {}, birth {}, entry {} group {}), program calls {}, calls {}",
            g.m.sum.insts.len(),
            kinds[0],
            kinds[1],
            fnkeyed,
            kinds[5],
            kinds[2],
            kinds[3] + kinds[4],
            g.m.sum.n_prog,
            g.m.calls.len()
        );
    }
    if cfg.profile {
        let mut v: Vec<_> = g.m.label_work.iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
        for (k, (es, ec, pops)) in v.iter().take(25) {
            eprintln!("perry: wpfacts labelwork {k} succ={es} cons={ec} pops={pops}");
        }
        let m = &g.m;
        let mut gs: Vec<_> = m
            .acc_groups
            .iter()
            .map(|((h, k), gr)| {
                let ng = match k {
                    model::Key::Named(a) => m.accessors.get(&(*h, *a)).map_or(0, |x| x.0.len()),
                    model::Key::Any => m.accessor_keys.get(h).map_or(0, |x| x.len()),
                };
                (
                    m.nodes[gr.ar as usize].pts.len(),
                    m.nodes[gr.gt as usize].pts.len(),
                    m.nodes[gr.ar as usize].succ.len(),
                    ng,
                    *h,
                    *k,
                )
            })
            .collect();
        gs.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (ar, gt, succ, ng, h, k) in gs.iter().take(12) {
            eprintln!(
                "perry: wpfacts accgroup {} key={} ar={ar} gt={gt} ar_succ={succ} accessors={ng}",
                m.tok_name(*h),
                m.key_name(*k)
            );
        }
    }
    g.m.dump_meta();
    let solve_ms = t1.elapsed().as_secs_f64() * 1000.0;
    // Decision 150: interpreted code is the ⊤ summary node, never a
    // program-wide kill. Every value it can receive (arguments and results
    // of ⊤ calls, globals (E20), the scope object of a direct eval) already
    // escapes to ⊤; a ⊤ call that may be the Function constructor only
    // connects already-⊤ values. Its writes to intrinsic prototypes are the
    // runtime's guard word (`ProgramFacts::interp_guard`).
    let mut interp_guard = None;
    if let Some(linked) = &cfg.dynamic_code {
        if g.m.dyn_eval.is_some() || g.m.top_calls > 0 {
            interp_guard = Some(format!(
                "E14 interpreter linked ({linked}) and reachable ({})",
                g.m.dyn_eval.clone().unwrap_or_else(|| format!(
                    "{} calls of unknown function values",
                    g.m.top_calls
                ))
            ));
        }
    }
    if cfg.dynamic_code_reach.globals && interp_guard.is_none() {
        let first = cfg
            .dynamic_code_reach
            .sites
            .iter()
            .next()
            .cloned()
            .unwrap_or_default();
        interp_guard = Some(format!("E14 dynamic_code_reach: {first}"));
    }
    let t2 = std::time::Instant::now();
    g.resolve_param_guards();
    let (mut templates, mut sites) = if g.m.aborted {
        (Vec::new(), Vec::new())
    } else {
        derive::derive(&mut g)
    };
    let mut bindings = bindings::collect(&g);
    if g.m.aborted {
        // Over budget: claim nothing (sound), keep the shapes for the dump.
        templates.clear();
        sites.clear();
        bindings.clear();
    }
    if let Some(spec) = &cfg.sabotage {
        dump::apply_sabotage(&mut templates, spec);
    }
    let derive_ms = t2.elapsed().as_secs_f64() * 1000.0;
    if cfg.profile {
        eprintln!(
            "perry: wpfacts post-solve: derive {derive_ms:.0} ms, {} markers' members settled",
            g.m.members_settled.borrow().len()
        );
    }
    let stats = Stats {
        modules: sorted.len(),
        templates: g.m.tmpls.len(),
        fact_units: templates.len(),
        nodes: g.m.nodes.len(),
        constraints: g.m.stats_cons,
        tokens: g.m.toks.len(),
        functions: g.m.funcs.len(),
        gen_ms,
        solve_ms,
        derive_ms,
        pops: g.m.pops,
        instances: g.m.sum.insts.len() as u64,
        shared_instances: g
            .m
            .sum
            .inst_key
            .keys()
            .filter(|k| k.0 == 1 && k.1 == summary::LAY_NONE)
            .count() as u64,
        composed: g.m.sum.ncomposed as u64,
        contexts: g.m.sum.ctx_nodes.len() as u64,
        work: g.m.work,
        aborted: g.m.aborted,
        checks,
        pts_entries: g.m.pts_entries,
        big_nodes: big_nodes(&g.m),
        top_calls: g.m.top_calls,
        profile: {
            let mut v: Vec<(String, u64, f64)> =
                g.m.profile
                    .iter()
                    .flatten()
                    .map(|(k, (n, d))| (k.to_string(), *n, d.as_secs_f64() * 1000.0))
                    .collect();
            v.sort_by(|a, b| b.2.total_cmp(&a.2));
            v
        },
    };
    let tok_names = if cfg.relations {
        (0..g.m.toks.len() as u32)
            .map(|t| g.m.tok_name(t))
            .collect()
    } else {
        Vec::new()
    };
    let fnproto_bind_written = if stats.aborted {
        Some("solve aborted".to_string())
    } else {
        g.m.fnproto_write.map(|r| g.m.reasons[r as usize].clone())
    };
    let super_unrelinked = if stats.aborted {
        Default::default()
    } else {
        g.m.toks
            .iter()
            .enumerate()
            .filter_map(|(t, kind)| {
                let model::TokKind::ClassObj(tm) = kind else {
                    return None;
                };
                let model::TmplKind::Class { class_id, .. } = g.m.tmpls[*tm as usize].kind else {
                    return None;
                };
                let origin = g.m.constructor_origin(*tm);
                let owner =
                    g.m.tok_index
                        .get(&model::TokKind::ClassObj(origin))
                        .copied()
                        .unwrap_or(t as u32);
                (!g.m.is_escaped(owner) && !g.m.super_relink.contains(&owner)).then_some(class_id)
            })
            .collect()
    };
    // Per-unit run: the classes this unit publishes as contracts, and what
    // it did beyond other units' contracts (design §11.2, §11.3).
    let reports = if cfg.unit { g.reports() } else { Vec::new() };
    let store_nodes = if cfg.unit {
        g.store_effects()
    } else {
        Vec::new()
    };
    let open_tm = if cfg.unit && !g.m.aborted {
        open_templates(&g)
    } else {
        Default::default()
    };
    let mut layouts_out: Vec<units::ClassContract> = Vec::new();
    // The class contracts: a class export under its name; a class given out
    // (DECISIONS 116) under its display only, a layout its readers find.
    let contracts_all: Vec<(Option<(String, String)>, units::ClassContract)> = if cfg.unit
        && !g.m.aborted
    {
        g.class_contracts
            .iter()
            .filter_map(|&(e, en, tm)| {
                let mi = match (e, &g.m.tmpls[tm as usize].kind) {
                    (Some((mi, _)), _) => mi,
                    (None, model::TmplKind::Class { module, .. }) => *module,
                    _ => return None,
                };
                let facts = templates.iter().find(|t| t.id == tm)?.clone();
                if chain_unsettled(&g, tm) {
                    return None;
                }
                let model::TmplKind::Class {
                    name,
                    class_id,
                    literal_shape,
                    ..
                } = g.m.tmpls[tm as usize].kind.clone()
                else {
                    return None;
                };
                let mut methods = Vec::new();
                let mut native_parent = None;
                let (statics, statics_open) = class_statics(&g, tm);
                // A static method the solve ran as a contract function
                // (`Model::publish_class`) publishes its contract.
                let static_fns: Vec<(String, units::FnContract)> = statics
                    .iter()
                    .filter_map(|(k, f)| {
                        let f = (*f)?;
                        g.m.contract_fns.contains(&f).then(|| {
                            (
                                k.clone(),
                                fn_contract_of(&g, &templates, f, &mut layouts_out, &open_tm),
                            )
                        })
                    })
                    .collect();
                let statics: Vec<(String, Option<String>)> = statics
                    .into_iter()
                    .map(|(k, f)| (k, f.map(|f| g.m.funcs[f as usize].name.clone())))
                    .collect();
                let ca = g.m.atoms.iter().position(|x| x == "constructor");
                let mut cur = Some(tm);
                while let Some(c) = cur {
                    if g.m.foreign.contains_key(&c) {
                        break;
                    }
                    let proto = g.m.tok_index.get(&model::TokKind::Proto(c)).copied();
                    let mut own: Vec<(String, String)> = g
                        .m
                        .fields
                        .iter()
                        .filter(|(&(h, k), _)| Some(h) == proto && Some(k as usize) != ca)
                        .filter_map(|(&(_, k), &n)| {
                            let ts = g.m.nodes[n as usize].pts.to_sorted();
                            let f = g.m.tok_fn(*ts.first()?);
                            (ts.len() == 1 && f != model::NO_FN).then(|| {
                                (
                                    g.m.atoms[k as usize].clone(),
                                    g.m.funcs[f as usize].name.clone(),
                                )
                            })
                        })
                        .filter(|(k, _)| !methods.iter().any(|(m, _): &(String, String)| m == k))
                        .collect();
                    own.sort();
                    methods.extend(own);
                    native_parent = g.m.tmpls[c as usize].native_parent.clone();
                    cur = g.m.tmpls[c as usize].parent;
                }
                // Methods and the constructor the solve ran as contract
                // functions (DECISIONS 57).
                let mut method_fns: Vec<(String, units::FnContract)> = Vec::new();
                for (k, _) in &methods {
                    let ka = g.m.atoms.iter().position(|x| x == k);
                    let mut cur = Some(tm);
                    while let Some(c) = cur {
                        if g.m.foreign.contains_key(&c) {
                            break;
                        }
                        let proto = g.m.tok_index.get(&model::TokKind::Proto(c)).copied();
                        let f = proto
                            .zip(ka)
                            .and_then(|(p, a)| g.m.fields.get(&(p, a as model::Atom)))
                            .and_then(|&n| {
                                let ts = g.m.nodes[n as usize].pts.to_sorted();
                                (ts.len() == 1).then(|| g.m.tok_fn(ts[0]))
                            });
                        if let Some(f) = f {
                            if f != model::NO_FN && g.m.contract_fns.contains(&f) {
                                method_fns.push((
                                    k.clone(),
                                    fn_contract_of(&g, &templates, f, &mut layouts_out, &open_tm),
                                ));
                            }
                            break;
                        }
                        cur = g.m.tmpls[c as usize].parent;
                    }
                }
                let ctor_fn = match g.m.tmpls[tm as usize].ctor {
                    _ if !g.m.ctor_published.contains(&tm) => None,
                    Some(f) => Some(fn_contract_of(
                        &g,
                        &templates,
                        f,
                        &mut layouts_out,
                        &open_tm,
                    )),
                    // No constructor code: the field initializers store the
                    // class's own keys into the receiver, nothing else.
                    None => Some(units::FnContract {
                        method: false,
                        name: format!("{name}.constructor"),
                        params: Vec::new(),
                        reach: Vec::new(),
                        reach_keys: Vec::new(),
                        variadic: false,
                        rest: false,
                        ret: Some(Vec::new()),
                        ret_params: Vec::new(),
                        ret_reach: Vec::new(),
                        ret_reach_keys: Vec::new(),
                        ret_unknown: false,
                        ret_value: Default::default(),
                        // A constructor with no code returns nothing `new`
                        // keeps: its result is the instance, as for one with
                        // code (the walk adds no undefined for falling off
                        // its end). Undefined here reached every `new` of the
                        // class in a reader and the arguments it published.
                        ret_prim: 0,
                        lexical_this: false,
                        closed: None,
                        this_use: 0,
                        this_reach: 0,
                        this_stores: Some(atoms_of(&g, &g.m.tmpls[tm as usize].birth_keys)),
                        this_calls: Vec::new(),
                        param_stores: Vec::new(),
                        param_flows: Vec::new(),
                        param_calls: Vec::new(),
                        param_handovers: Vec::new(),
                        param_cells: Vec::new(),
                    }),
                };
                let key = e.map(|(_, a)| {
                    (
                        sorted[mi as usize].0.to_string(),
                        g.m.atoms[a as usize].clone(),
                    )
                });
                Some((
                    key,
                    units::ClassContract {
                        display: g.m.tmpls[tm as usize].display.clone(),
                        name,
                        class_id,
                        literal_shape,
                        facts,
                        methods,
                        native_parent,
                        parent: foreign_parent(&g, tm),
                        field_fns: Vec::new(),
                        // Its instances' fields leave key by key (DECISIONS 88).
                        field_layouts: nested_layouts(
                            &g,
                            &templates,
                            tm,
                            &mut layouts_out,
                            &open_tm,
                        ),
                        field_tas: field_ta_contracts(&g, tm),
                        field_values: field_value_contracts(
                            &g,
                            &templates,
                            tm,
                            &mut layouts_out,
                            &open_tm,
                        ),
                        open_keys: open_tm.contains(&tm),
                        statics,
                        statics_open,
                        static_fns,
                        static_values: Vec::new(),
                        closed: closed_of(&g, sorted[0].0, mi, tm),
                        forwards: forwards_of(&g, mi, tm),
                        method_fns,
                        ctor_fn,
                        birth_keys: atoms_of(&g, &g.m.tmpls[tm as usize].birth_keys),
                        definite: atoms_of(&g, &g.m.tmpls[tm as usize].definite),
                        ctor_leaks: g.chain_leaks(tm),
                        binding_prim: match e {
                            Some(_) => g.m.nodes[en as usize].prim,
                            None => 0,
                        },
                    },
                ))
            })
            .collect()
    } else {
        Vec::new()
    };
    let (contracts_out, given_out): (Vec<_>, Vec<_>) =
        contracts_all.into_iter().partition(|(k, _)| k.is_some());
    let contracts_out: Vec<((String, String), units::ClassContract)> = contracts_out
        .into_iter()
        .map(|(k, c)| (k.expect("an export"), c))
        .collect();
    let mut given_out: Vec<units::ClassContract> = given_out.into_iter().map(|(_, c)| c).collect();
    // DECISIONS 173: the static members `Gen::contract_check` accepted, by
    // class, with the contracts of what they hold.
    let mut contracts_out = contracts_out;
    if cfg.unit && !g.m.aborted {
        let mut sv: Vec<(String, String, units::ValueContract)> = Vec::new();
        for &(tm, a, n) in &g.static_values {
            let vc = value_contract_of(
                &g,
                &templates,
                n,
                &contracts_out,
                &mut layouts_out,
                &open_tm,
            );
            sv.push((
                g.m.tmpls[tm as usize].display.clone(),
                g.m.atoms[a as usize].clone(),
                vc,
            ));
        }
        sv.sort_by(|x, y| (&x.0, &x.1).cmp(&(&y.0, &y.1)));
        for c in contracts_out
            .iter_mut()
            .map(|(_, c)| c)
            .chain(given_out.iter_mut())
        {
            c.static_values = sv
                .iter()
                .filter(|(d, k, _)| d == &c.display && c.statics.iter().any(|(q, _)| q == k))
                .map(|(_, k, v)| (k.clone(), v.clone()))
                .collect();
        }
    }
    // Its function contracts and the layouts they return (design §11.2).
    let fn_contracts_out: Vec<((String, String), units::FnContract)> = if cfg.unit && !g.m.aborted {
        let mut out = Vec::new();
        for &((mi, a), f) in &g.fn_contracts {
            let key = (
                sorted[mi as usize].0.to_string(),
                g.m.atoms[a as usize].clone(),
            );
            out.push((
                key,
                fn_contract_of(&g, &templates, f, &mut layouts_out, &open_tm),
            ));
        }
        out
    } else {
        Vec::new()
    };
    let fns_defined: Vec<String> = if cfg.unit && !g.m.aborted {
        let mut out: Vec<String> =
            g.m.contract_fns
                .iter()
                .filter(|&&f| g.m.funcs[f as usize].func_id != u32::MAX)
                .map(|&f| format!("fn:{}", fn_contract_name(&g, f).0))
                .collect();
        out.sort();
        out
    } else {
        Vec::new()
    };
    // DECISIONS 106: every function some contract may describe goes out
    // under its name, its owner's contract.
    let own_fns_out: Vec<units::FnContract> = if cfg.unit && !g.m.aborted {
        let mut fs: Vec<model::FnIdx> =
            g.m.contract_fns
                .iter()
                .copied()
                .filter(|&f| g.m.funcs[f as usize].func_id != u32::MAX)
                .collect();
        fs.sort_unstable();
        fs.into_iter()
            .map(|f| fn_contract_of(&g, &templates, f, &mut layouts_out, &open_tm))
            .collect()
    } else {
        Vec::new()
    };
    // Its value contracts (design §12): per published value binding, the
    // contracts of what it may hold (`Gen::contract_check` accepted them).
    let values_out: Vec<((String, String), units::ValueContract)> = if cfg.unit && !g.m.aborted {
        let mut out = Vec::new();
        for &((mi, a), n) in &g.value_contracts {
            let vc = value_contract_of(
                &g,
                &templates,
                n,
                &contracts_out,
                &mut layouts_out,
                &open_tm,
            );
            let key = (
                sorted[mi as usize].0.to_string(),
                g.m.atoms[a as usize].clone(),
            );
            out.push((key, vc));
        }
        out
    } else {
        Vec::new()
    };
    // Its store effects with their value contracts (design §17.16 (c)): an
    // undescribed value (`Gen::effect_bad`) or a solve over the bound is
    // unknown, of any kind.
    let stores_out: Vec<(String, String, units::ValueContract)> = store_nodes
        .into_iter()
        .map(|(d, k, nodes)| {
            let mut vc = units::ValueContract::default();
            for n in nodes {
                if g.m.aborted || g.effect_bad.contains(&n) {
                    vc.unknown = true;
                    vc.prim = model::prim::ALL;
                } else {
                    vc.join(&value_contract_of(
                        &g,
                        &templates,
                        n,
                        &contracts_out,
                        &mut layouts_out,
                        &open_tm,
                    ));
                }
            }
            (d, k, vc)
        })
        .collect();
    // What it writes into each cell (design §17.16 (b)): what the cell node
    // holds, the other units' writes it read included (a forwarded stand-in
    // is its owner's contract, a join adds nothing): the cells grow
    // monotonically to the fixpoint. Undescribed (`Gen::cell_bad`) or over
    // the bound, unknown.
    let mut cells_out: Vec<(String, units::ValueContract)> = g
        .cells
        .iter()
        .filter_map(|(k, n)| {
            let node = &g.m.nodes[*n as usize];
            if g.m.aborted || g.cell_bad.contains(n) {
                let vc = units::ValueContract {
                    unknown: true,
                    prim: model::prim::ALL,
                    ..Default::default()
                };
                return Some((k.clone(), vc));
            }
            // DECISIONS 162: an own parameter's marker in it is the callers'
            // argument, which their units store there themselves.
            let toks: Vec<model::Tok> = node
                .pts
                .to_sorted()
                .into_iter()
                .filter(|t| !g.cell_flow_toks.contains(t))
                .collect();
            let prim = node.prim;
            if toks.is_empty() && prim == 0 {
                return None;
            }
            let vc = value_contract_of_toks(
                &g,
                &templates,
                prim,
                toks,
                &contracts_out,
                &mut layouts_out,
                &open_tm,
            );
            Some((k.clone(), vc))
        })
        .collect();
    // Its globals as unmodeled code may write them (the global object
    // reached ⊤, a computed key was written, a key named after the
    // boundary): every unit's global cells read ⊤.
    let gt =
        g.m.tok_index
            .get(&model::TokKind::Builtin(model::BuiltinNs::GlobalThis))
            .copied();
    if g.m.global_cells
        && (g.m.aborted
            || g.m.global_in_top
            || g.m.global_late
            || gt.is_some_and(|t| g.m.native_writes.contains_key(&(t, None))))
    {
        let vc = units::ValueContract {
            unknown: true,
            prim: model::prim::ALL,
            ..Default::default()
        };
        cells_out.push(("global:*".to_string(), vc));
    }
    // DECISIONS 94: an array unmodeled code may write (it escaped): every
    // unit's elements of it read ⊤.
    // A named key stored into one: every unit's named fields of it read ⊤.
    for (key, t) in &g.arr_cells {
        let vc = units::ValueContract {
            unknown: true,
            prim: model::prim::ALL,
            ..Default::default()
        };
        if g.m.aborted || g.m.is_escaped(*t) {
            cells_out.retain(|(k, _)| k != key);
            cells_out.push((key.clone(), vc.clone()));
        }
        if g.m.aborted || g.m.is_escaped(*t) || g.m.arr_named.contains(t) {
            cells_out.push((format!("{key}#named"), vc));
        }
    }
    // What it writes into other units' module-scope bindings (`Gen::local_write`):
    // per binding (its unit's path, its name) a value contract, as of a
    // published binding; ⊤ for one no contract describes, and for every
    // binding it writes when the solve stopped at the bound.
    let writes_out: Vec<((String, String), units::ValueContract)> = if cfg.unit {
        let unknown = units::ValueContract {
            unknown: true,
            prim: model::prim::ALL,
            ..Default::default()
        };
        g.written_keys()
            .into_iter()
            .filter_map(|(id, key)| {
                if g.m.aborted || g.write_top.contains(&id) {
                    return Some((key, unknown.clone()));
                }
                let &n = g.binding_writes.get(&id)?;
                let vc = value_contract_of(
                    &g,
                    &templates,
                    n,
                    &contracts_out,
                    &mut layouts_out,
                    &open_tm,
                );
                Some((key, vc))
            })
            .collect()
    } else {
        Vec::new()
    };
    // What its calls pass to the other units' closed functions (decision 13).
    let args_out: Vec<(String, units::ArgContract)> = match g.view {
        Some(v) if cfg.unit && !g.m.aborted => {
            let mut out: Vec<(String, units::ArgContract)> = Vec::new();
            for nm in &v.closed_refs {
                // An import binding is made when the unit's code names it
                // (115-CWL3): one never made is named by no code of this
                // unit, so no call of it is here (DECISIONS 156). Reached
                // otherwise (a namespace key, a value it got), its stand-in
                // publishes below.
                let Some(&n) = g.mods[0].imports.get(nm) else {
                    continue;
                };
                let toks = g.m.nodes[n as usize].pts.to_sorted();
                // A closed class through its contract: `ctor_args_out`.
                if !toks.is_empty()
                    && toks.iter().all(|&t| {
                        matches!(g.m.toks[t as usize], model::TokKind::ClassObj(tm)
                            if g.closed_ctors.iter().any(|c| c.1 == tm))
                    })
                {
                    continue;
                }
                out.push((
                    nm.clone(),
                    arg_contract_of(
                        &g,
                        &templates,
                        toks,
                        &contracts_out,
                        &mut layouts_out,
                        &open_tm,
                    ),
                ));
            }
            // A closed function this unit got as an argument value
            // (decision 19): what its calls of it pass.
            let mut got: Vec<(String, model::Tok)> = g
                .stand_in_fns
                .iter()
                .filter(|((_, holder), _)| holder.is_none())
                .filter_map(|(_, &(_, t))| Some((g.stand_in_fc.get(&t)?.closed.clone()?, t)))
                .filter(|(nm, _)| {
                    !v.closed_refs.contains(nm) || !g.mods[0].imports.contains_key(nm)
                })
                .collect();
            got.sort();
            for (nm, t) in got {
                let ac = arg_contract_of(
                    &g,
                    &templates,
                    vec![t],
                    &contracts_out,
                    &mut layouts_out,
                    &open_tm,
                );
                out.push((nm, ac));
            }
            out
        }
        _ => Vec::new(),
    };
    // What its constructions of closed classes pass (design §18.13): the
    // stand-in constructor's parameters. A stand-in class object that
    // escaped here is this unit's report (`Gen::reports`): the class is
    // broken, its constructor's parameters ⊤.
    let ctor_args_out: Vec<((String, String), units::ArgContract)> = match g.view {
        Some(_) if cfg.unit && !g.m.aborted => g
            .closed_ctors
            .iter()
            .map(|(key, _, ft)| {
                let ac = arg_contract_of(
                    &g,
                    &templates,
                    vec![*ft],
                    &contracts_out,
                    &mut layouts_out,
                    &open_tm,
                );
                (key.clone(), ac)
            })
            .collect(),
        _ => Vec::new(),
    };
    // DECISIONS 107: per method of another unit, the receivers this unit
    // runs it on (`Model::this_admitted`), published to the method's unit.
    // A receiver marker or a published receiver token stands for instances
    // of classes further units may extend: unknown.
    let this_args_out: Vec<(String, units::ValueContract)> = match g.view {
        Some(_) if cfg.unit && !g.m.aborted => {
            let mut by: Vec<(String, Vec<model::Tok>)> = Vec::new();
            for &(this, t) in &g.m.this_admitted {
                let Some(fc) =
                    g.m.this_contract_fns
                        .get(this as usize)
                        .and_then(|ft| g.stand_in_fc.get(ft))
                else {
                    continue;
                };
                match by.iter_mut().find(|(n, _)| *n == fc.name) {
                    Some((_, v)) if !v.contains(&t) => v.push(t),
                    Some(_) => {}
                    None => by.push((fc.name.clone(), vec![t])),
                }
            }
            by.sort();
            by.into_iter()
                .map(|(nm, toks)| {
                    let opaque = toks.iter().any(|t| {
                        g.m.param_marker.contains_key(t)
                            || g.m.recv_class.contains_key(t)
                            || g.m.reach_marker.contains(t)
                    });
                    let mut vc = value_contract_of_toks(
                        &g,
                        &templates,
                        0,
                        toks,
                        &contracts_out,
                        &mut layouts_out,
                        &open_tm,
                    );
                    vc.unknown |= opaque;
                    (nm, vc)
                })
                .collect()
        }
        _ => Vec::new(),
    };
    // DECISIONS 165: the methods of other units this unit runs: a call bound
    // their stand-in (a method call or a plain one, however this unit got
    // the function: a class contract, an import, a value). Published to the
    // method's unit, which reads a method no unit runs as having no
    // argument of another unit.
    let method_runs_out: Vec<String> = match g.view {
        Some(_) if cfg.unit && !g.m.aborted => {
            let mut v: Vec<String> = g
                .stand_in_fc
                .iter()
                .filter(|(&t, fc)| fc.method && g.m.called.contains(&g.m.tok_fn(t)))
                .map(|(_, fc)| fc.name.clone())
                .chain(
                    g.m.stub_methods
                        .iter()
                        .filter(|f| g.m.called.contains(f))
                        .map(|&f| g.m.funcs[f as usize].name.clone()),
                )
                .chain(unnamed_methods(&g.m).map(|f| g.m.funcs[f as usize].name.clone()))
                .collect();
            v.sort();
            v.dedup();
            v
        }
        _ => Vec::new(),
    };
    // DECISIONS 148: what this unit passes to the methods of other units it
    // runs (the 107 rule for parameters): per contract name, what its calls
    // of the stand-ins bind (`arg_contract_of`, as for a closed function,
    // decision 13). A stub method (no function contract) or one called
    // under no name (through ⊤) passes what no contract describes: `top`.
    let method_args_out: Vec<(String, units::ArgContract)> = match g.view {
        Some(_) if cfg.unit && !g.m.aborted => {
            let mut by: Vec<(String, Vec<model::Tok>)> = Vec::new();
            for (&t, fc) in &g.stand_in_fc {
                if !fc.method || !g.m.called.contains(&g.m.tok_fn(t)) {
                    continue;
                }
                match by.iter_mut().find(|(n, _)| *n == fc.name) {
                    Some((_, v)) => v.push(t),
                    None => by.push((fc.name.clone(), vec![t])),
                }
            }
            by.sort();
            let mut out: Vec<(String, units::ArgContract)> = by
                .into_iter()
                .map(|(nm, mut toks)| {
                    toks.sort_unstable();
                    let ac = arg_contract_of(
                        &g,
                        &templates,
                        toks,
                        &contracts_out,
                        &mut layouts_out,
                        &open_tm,
                    );
                    (nm, ac)
                })
                .collect();
            let undescribed: Vec<String> =
                g.m.stub_methods
                    .iter()
                    .filter(|f| g.m.called.contains(f))
                    .map(|&f| g.m.funcs[f as usize].name.clone())
                    .chain(unnamed_methods(&g.m).map(|f| g.m.funcs[f as usize].name.clone()))
                    .collect();
            for nm in undescribed {
                match out.iter_mut().find(|(n, _)| *n == nm) {
                    Some((_, ac)) => ac.top = true,
                    None => out.push((
                        nm,
                        units::ArgContract {
                            params: Vec::new(),
                            top: true,
                        },
                    )),
                }
            }
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        }
        _ => Vec::new(),
    };
    // A contract class a contract result or value names is published
    // whole (methods, statics): its readers call them on what they get.
    for l in layouts_out.iter_mut() {
        if let Some(c) = contracts_out
            .iter()
            .map(|(_, c)| c)
            .chain(&given_out)
            .find(|c| c.display == l.display)
        {
            *l = c.clone();
        }
    }
    // DECISIONS 132: every class this unit publishes goes out under its
    // display too, the owner's contract of this round. A reader holding a
    // copy (a value or argument contract describes it as the publishing
    // unit saw it) builds its stand-in over the owner's
    // (`Gen::other_layout`), a read the driver tracks: when the owner's
    // contract changes, the reader is solved again.
    for c in contracts_out.iter().map(|(_, c)| c).chain(&given_out) {
        if !layouts_out.iter().any(|l| l.display == c.display) {
            layouts_out.push(c.clone());
        }
    }
    // The other units' parents those contracts name go out beside them,
    // with the contract this unit holds for them (DECISIONS 104).
    let mut i = 0;
    let mut parents: Vec<String> = contracts_out
        .iter()
        .map(|(_, c)| c)
        .chain(&given_out)
        .filter_map(|c| c.parent.clone())
        .collect();
    while i < layouts_out.len() || !parents.is_empty() {
        if let Some(d) = parents.pop() {
            if layouts_out.iter().any(|c| c.display == d) {
                continue;
            }
            if let Some(cc) = g.stand_in_cc.values().find(|c| c.display == d) {
                layouts_out.push(cc.clone());
            }
            continue;
        }
        parents.extend(layouts_out[i].parent.clone());
        i += 1;
    }
    // Forwarded: another unit's layout this unit holds a stand-in of. A
    // class this unit defines is its own publication, whatever stand-in
    // it also holds (DECISIONS 133).
    let layouts_fwd: Vec<String> = layouts_out
        .iter()
        .filter(|c| g.stand_in_cc.values().any(|s| s.display == c.display))
        .filter(|c| {
            !contracts_out
                .iter()
                .map(|(_, o)| o)
                .chain(&given_out)
                .any(|o| o.display == c.display)
        })
        .map(|c| c.display.clone())
        .collect();
    let mut pf = ProgramFacts {
        super_unrelinked,
        fnproto_bind_written,
        module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
        templates,
        sites,
        stats,
        interp_guard,
        dynamic_code_reach: cfg.dynamic_code_reach.clone(),
        sabotage: cfg.sabotage.clone(),
        relations: g.m.relations.take(),
        tok_names,
        bindings,
        units: Vec::new(),
        unit_io: Vec::new(),
        rounds: 0,
        broken: Vec::new(),
        round_work: Vec::new(),
        compares: 0,
        contracts_out,
        fn_contracts_out,
        fns_defined,
        layouts_out,
        values_out,
        args_out,
        ctor_args_out,
        reports,
        stores_out,
        cells_out,
        cells_read: std::mem::take(&mut g.cells_read),
        layouts_read: std::mem::take(&mut g.layouts_read),
        own_fns_out,
        fns_read: std::mem::take(&mut g.fns_read),
        this_args_out,
        method_runs_out,
        method_args_out,
        layouts_fwd,
        writes_out,
        module_digests: Vec::new(),
    };
    pf.module_digests = dump::module_digests(&pf);
    pf
}

/// Per-unit facts (decision 151, phase P1): one solve per module, every
/// other module unknown compiled code ([`FactsConfig::unit`]), the bounds
/// per unit. A module's sites, bindings and templates come from its own
/// unit; a unit over a bound claims nothing for its module (sound), the
/// others keep theirs. Program-wide flags are the union over the units.
pub fn compute_units(modules: &[(String, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut pf = units::run(&sorted, cfg);
    pf.module_digests = dump::module_digests(&pf);
    pf
}

impl ProgramFacts {
    fn module_index(&self, path: &str) -> Option<usize> {
        self.module_paths
            .binary_search_by(|p| p.as_str().cmp(path))
            .ok()
    }

    /// The digest of the facts module `path` can observe (its own templates
    /// and the templates in its sites' receiver sets). Goes into the
    /// module's object-cache key (design section 7).
    pub fn digest_for(&self, path: &str) -> u64 {
        match self.module_index(path) {
            Some(i) => self.module_digests[i],
            None => 0,
        }
    }

    /// The contract-build claims for the class templates `path` defines:
    /// the `.perry_facts` payload (empty when nothing is claimed).
    pub fn contract_section(&self, path: &str) -> Vec<u8> {
        match self.module_index(path) {
            Some(i) => dump::contract_section(self, i as u32),
            None => Vec::new(),
        }
    }

    /// The classes whose layout facts hold for every instance, by class id.
    /// A class id names one runtime class, and structurally identical
    /// literal classes of several modules share it: its facts hold only when
    /// every template with that id is prefix-stable and they agree.
    pub fn stable_classes(&self) -> std::collections::HashMap<u32, &TemplateFacts> {
        let mut by_class: std::collections::HashMap<u32, Vec<&TemplateFacts>> = Default::default();
        for t in &self.templates {
            if let Some(c) = t.class_id {
                by_class.entry(c).or_default().push(t);
            }
        }
        by_class
            .into_iter()
            .filter(|(_, ts)| {
                ts.iter().all(|t| t.prefix_stable) && ts.iter().all(|t| t.fields == ts[0].fields)
            })
            .map(|(c, ts)| (c, ts[0]))
            .collect()
    }

    /// Receiver facts of module `path`'s bindings (empty when unknown).
    pub fn bindings_for(&self, path: &str) -> Option<&ModuleBindings> {
        self.bindings.get(self.module_index(path)?)
    }

    /// The human/oracle-readable facts dump.
    pub fn dump_text(&self) -> String {
        dump::dump_text(self)
    }

    /// The constraint relations for `scripts/wpfacts_recompute.py`.
    pub fn relations_text(&self) -> Option<String> {
        dump::relations_text(self)
    }
}

fn big_nodes(m: &model::Model) -> Vec<(String, usize, String)> {
    let mut sizes: Vec<(usize, usize)> = m
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.pts.len(), i))
        .filter(|(l, _)| *l > 64)
        .collect();
    sizes.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let count_big = sizes.len();
    let mut out: Vec<(String, usize, String)> = sizes
        .into_iter()
        .take(20)
        .map(|(len, i)| {
            let mut mix: std::collections::BTreeMap<&'static str, usize> = Default::default();
            for t in m.nodes[i].pts.to_sorted() {
                let k = match &m.toks[t as usize] {
                    model::TokKind::Top => "top",
                    model::TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
                        model::TmplKind::Class { .. } => "class",
                        model::TmplKind::Lit { .. } => "lit",
                        model::TmplKind::FnInst { .. } => "fninst",
                        model::TmplKind::Arr { .. } => "arr",
                        _ => "bobj",
                    },
                    model::TokKind::Func(_, _) => "func",
                    model::TokKind::Bound(_) => "bound",
                    model::TokKind::ClassObj(_) => "classobj",
                    model::TokKind::Proto(_) => "proto",
                    _ => "builtin",
                };
                *mix.entry(k).or_default() += 1;
            }
            let label = m.labels.get(i).copied().unwrap_or("?");
            // A field node names its holder and key; any node its function.
            let mut owner = m
                .fields
                .iter()
                .find(|(_, &n)| n as usize == i)
                .map(|(&(t, a), _)| format!("({}.{})", m.tok_name(t), m.atoms[a as usize]))
                .unwrap_or_default();
            if let Some(&f) = m.node_fn.get(i).filter(|&&f| f != model::NO_FN) {
                owner.push_str(&format!("@{}", m.funcs[f as usize].name));
            }
            (
                format!("{label}#{i}{owner}"),
                len,
                mix.iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
        .collect();
    out.push((format!("nodes>64:{count_big}"), 0, String::new()));
    out
}

/// `PERRY_PROGRAM_FACTS=0` turns the pass off (every fact off; digest 0).
pub fn enabled_from_env() -> bool {
    !matches!(
        std::env::var("PERRY_PROGRAM_FACTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}
