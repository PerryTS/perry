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
    /// The program's CommonJS closed-world fact
    /// (`perry_hir::program_is_cjs_closed`): no module can reach the loader,
    /// so every CommonJS factory runs once (the premise of the run-once
    /// view's factory case, `decl_units::run_once_flat`).
    pub cjs_closed: bool,
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
    /// What it does to the objects every unit shares (`model::SharedWrite`).
    pub(crate) shared_out: Vec<model::SharedWrite>,
    /// What its solve can observe of them (`model::SharedReads`).
    pub(crate) shared_reads: model::SharedReads,
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
            // Another unit's layout: its owner publishes it.
            if g.forwarded_layout(itm) {
                ds.push(display);
                continue;
            }
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
                    field_fns: Vec::new(),
                    field_layouts: Vec::new(),
                    open_keys: open_tm.contains(&itm),
                    statics: Vec::new(),
                    statics_open: false,
                    static_fns: Vec::new(),
                    closed: None,
                });
                let nl = nested_layouts(g, templates, itm, layouts_out, open_tm);
                if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                    c.field_layouts = nl;
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
        Some(k) if k.contains('#') => format!("{}#{fid}", fi.name),
        _ => fi.name.clone(),
    };
    (name, closed)
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
    debug_assert!(
        (0..fi.params.len() as u32).all(|i| g.m.marker_of.contains_key(&(f, i))),
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
    // its uses.
    let params: Vec<u8> = uses(false)
        .into_iter()
        .enumerate()
        .map(
            |(i, u)| match g.m.closed_described.get(&f).and_then(|d| d.get(i)) {
                Some(true) => u & !model::MARK_ESCAPE,
                _ => u,
            },
        )
        .collect();
    // What each parameter reaches, by the key it was read under (§18.4).
    let marker_use = |t: model::Tok| g.m.marker_use.get(&t).copied().unwrap_or(0);
    let reach: Vec<u8> = (0..fi.params.len() as u32)
        .map(|i| {
            let mk = g.m.marker_of[&(f, i)];
            g.m.marker_reach.get(&mk).map_or(0, |&r| marker_use(r))
        })
        .collect();
    let reach_keys: Vec<Vec<(String, u8)>> = (0..fi.params.len() as u32)
        .map(|i| {
            let mk = g.m.marker_of[&(f, i)];
            let mut v: Vec<(model::Atom, u8)> =
                g.m.keyed_uses.get(&mk).cloned().unwrap_or_default();
            for &(a, r) in g.m.keyed_reach.get(&mk).into_iter().flatten() {
                match v.iter_mut().find(|(k, _)| *k == a) {
                    Some((_, u)) => *u |= marker_use(r),
                    None => v.push((a, marker_use(r))),
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
    let mut ret_params: Vec<u32> = Vec::new();
    let mut ret_reach: Vec<u32> = Vec::new();
    let mut ret_unknown = false;
    let ret_prim = rets.iter().fold(0, |p, &n| p | g.m.nodes[n as usize].prim);
    let ret = (!g.ret_top.contains(&f) && !rets.is_empty()).then(|| {
        let mut ds: Vec<String> = Vec::new();
        for n in rets {
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let toks = g.m.nodes[n as usize].pts.to_sorted();
            for t in toks.into_iter().flat_map(|t| g.m.marker_members(t)) {
                // An escaped marker reads as ⊤ (DECISIONS 42).
                if matches!(
                    g.m.toks[t as usize],
                    model::TokKind::Top | model::TokKind::BuiltinFn
                ) || g.m.marker_is_top(t)
                {
                    ret_unknown = true;
                    continue;
                }
                if let Some(&(_, i)) = g.m.param_marker.get(&t) {
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
                    if !layouts_out.iter().any(|c| c.display == display) {
                        layouts_out.push(units::ClassContract {
                            display: display.clone(),
                            name,
                            class_id,
                            literal_shape,
                            facts: facts.clone(),
                            methods: Vec::new(),
                            native_parent: None,
                            field_fns: Vec::new(),
                            field_layouts: Vec::new(),
                            open_keys: open_tm.contains(&tm),
                            statics: Vec::new(),
                            statics_open: false,
                            static_fns: Vec::new(),
                            closed: None,
                        });
                        let nl = nested_layouts(g, templates, tm, layouts_out, open_tm);
                        if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                            c.field_layouts = nl;
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
    let (name, closed) = fn_contract_name(g, f);
    units::FnContract {
        name,
        closed,
        params,
        reach,
        reach_keys,
        // A rest parameter takes only the arguments past the others
        // (design §18.11); `arguments` aliases every argument.
        variadic: fi.has_arguments,
        rest: fi.rest.is_some() && !fi.has_arguments,
        ret,
        ret_params: {
            ret_params.sort();
            ret_params
        },
        ret_reach: {
            ret_reach.sort();
            ret_reach
        },
        ret_unknown,
        ret_prim,
        lexical_this: fi.lexical_this || !reads_this(g, f),
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

/// The value contract of what node `n` may hold (`Gen::contract_check`
/// accepted it): its literal layouts and classes by their contracts, its
/// functions by theirs, another unit's stand-ins by their owners', ⊤ and
/// the primitive kinds. `back`: an effect's (`Gen::value_contract_holds`),
/// whose objects' fields left whole: no nested layouts.
fn value_contract_of_node(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    n: model::NodeId,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
    back: bool,
) -> units::ValueContract {
    let node = &g.m.nodes[n as usize];
    let mut vc = units::ValueContract {
        layouts: Vec::new(),
        classes: Vec::new(),
        fns: Vec::new(),
        unknown: false,
        shared: Vec::new(),
        prim: node.prim,
    };
    for t in node.pts.to_sorted() {
        // A runtime-owned object, by name (`Model::named_shared`).
        if let Some(s) = g.m.named_shared(t) {
            if !vc.shared.contains(&s) {
                vc.shared.push(s);
            }
        }
        let (tm, class) = match g.m.toks[t as usize] {
            model::TokKind::Top | model::TokKind::BuiltinFn => {
                vc.unknown = true;
                continue;
            }
            model::TokKind::Obj(tm, _) => (tm, false),
            model::TokKind::ClassObj(tm) => (tm, true),
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
                let (name, class_id, literal_shape) = match g.m.tmpls[tm as usize].kind.clone() {
                    model::TmplKind::Class {
                        name,
                        class_id,
                        literal_shape,
                        ..
                    } => (name, class_id, literal_shape),
                    // An enum's object (§18.2): a literal layout with
                    // no class (nothing guards it, so no class id).
                    model::TmplKind::Lit {
                        enum_image: true, ..
                    } => (display.clone(), 0, true),
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
                    field_fns: Vec::new(),
                    field_layouts: Vec::new(),
                    open_keys: open_tm.contains(&tm),
                    statics: Vec::new(),
                    statics_open: false,
                    static_fns: Vec::new(),
                    closed: None,
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
            cc.field_fns = literal_field_fns(
                &g,
                templates,
                &node.pts.to_sorted(),
                tm,
                layouts_out,
                open_tm,
            );
            if !back {
                cc.field_layouts = nested_layouts(g, templates, tm, layouts_out, open_tm);
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
    vc.shared.sort();
    vc
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
        if fi.has_arguments && !g.closed_ctors.iter().any(|c| c.2 == t) {
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
            for t in toks.into_iter().flat_map(|t| g.m.marker_members(t)) {
                // A runtime-owned object, by name (`Model::named_shared`); a
                // builtin function value is unknown there besides.
                if let Some(s) = g.m.named_shared(t) {
                    let unknown_too = s == model::Shared::BuiltinFn;
                    if !vc.shared.contains(&s) {
                        vc.shared.push(s);
                    }
                    if !unknown_too {
                        continue;
                    }
                }
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
                let display = &g.m.tmpls[tm as usize].display;
                // A closed caller passing its own parameter on: what its
                // callers pass is beside the marker (decision 13).
                if g.m
                    .param_marker
                    .get(&t)
                    .is_some_and(|(f, _)| g.m.closed_params.contains_key(f))
                {
                    continue;
                }
                if g.m.param_marker.contains_key(&t) || g.unit_in.is_some_and(|u| u.broken(display))
                {
                    vc.unknown = true;
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
                            field_fns: Vec::new(),
                            field_layouts: Vec::new(),
                            open_keys: open_tm.contains(&tm),
                            statics: Vec::new(),
                            statics_open: false,
                            static_fns: Vec::new(),
                            closed: None,
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
    for p in &mut ac.params {
        p.shared.sort();
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
        g.hold_binding_writes();
        g.unit_boundary(published);
        // The other units' writes of the shared objects (`model::SharedWrite`).
        if let Some(p) = published {
            for w in &p.shared {
                g.m.replay_shared(w);
            }
        }
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
        g.contract_check()
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
        // One count per instance key kind (`Summaries::inst_key`).
        let mut kinds = [0usize; 7];
        let mut fnkeyed = 0;
        for &(k, l, _) in g.m.sum.inst_key.keys() {
            kinds[k as usize] += 1;
            fnkeyed += (k == 1 && l == summary::LAY_NONE) as usize;
        }
        eprintln!(
            "perry: wpfacts instances {} (site {} layout {} of them function-keyed {}, birth {}, entry {} group {} for-in {}), program calls {}, calls {}",
            g.m.sum.insts.len(),
            kinds[0],
            kinds[1],
            fnkeyed,
            kinds[5],
            kinds[2],
            kinds[3] + kinds[4],
            kinds[6],
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
    let open_tm = if cfg.unit && !g.m.aborted {
        open_templates(&g)
    } else {
        Default::default()
    };
    let mut layouts_out: Vec<units::ClassContract> = Vec::new();
    let contracts_out: Vec<((String, String), units::ClassContract)> = if cfg.unit && !g.m.aborted {
        g.class_contracts
            .iter()
            .filter_map(|&((mi, a), _, tm)| {
                let facts = templates.iter().find(|t| t.id == tm)?.clone();
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
                let key = (
                    sorted[mi as usize].0.to_string(),
                    g.m.atoms[a as usize].clone(),
                );
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
                        field_fns: Vec::new(),
                        field_layouts: Vec::new(),
                        open_keys: open_tm.contains(&tm),
                        statics,
                        statics_open,
                        static_fns,
                        closed: closed_of(&g, sorted[0].0, mi, tm),
                    },
                ))
            })
            .collect()
    } else {
        Vec::new()
    };
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
    // Its value contracts (design §12): per published value binding, the
    // contracts of what it may hold (`Gen::contract_check` accepted them).
    let values_out: Vec<((String, String), units::ValueContract)> = if cfg.unit && !g.m.aborted {
        let mut out = Vec::new();
        for &((mi, a), n) in &g.value_contracts {
            let vc = value_contract_of_node(
                &g,
                &templates,
                n,
                &contracts_out,
                &mut layouts_out,
                &open_tm,
                false,
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
    // Its store effects into other units' literal layouts and class
    // instances (decision 17): per layout display and key the value contract
    // of what it stores, ⊤ for a value no contract describes and for every
    // store when the solve stopped at the bound.
    let stores_out: Vec<(String, String, units::ValueContract)> = if cfg.unit {
        let unknown = units::ValueContract {
            unknown: true,
            prim: model::prim::ALL,
            ..Default::default()
        };
        let mut stored: Vec<(model::TmplId, model::Atom, model::NodeId)> =
            g.m.foreign_stores.iter().copied().collect();
        stored.sort_unstable();
        let mut by: Vec<((model::TmplId, model::Atom), units::ValueContract)> = Vec::new();
        for (tm, a, n) in stored {
            let vc = if g.m.aborted || g.store_top.contains(&(tm, a)) {
                unknown.clone()
            } else {
                value_contract_of_node(
                    &g,
                    &templates,
                    n,
                    &contracts_out,
                    &mut layouts_out,
                    &open_tm,
                    true,
                )
            };
            match by.last_mut() {
                Some((k, e)) if *k == (tm, a) => e.join(&vc),
                _ => by.push(((tm, a), vc)),
            }
        }
        by.into_iter()
            .map(|((tm, a), vc)| {
                (
                    g.m.tmpls[tm as usize].display.clone(),
                    g.m.atoms[a as usize].clone(),
                    vc,
                )
            })
            .collect()
    } else {
        Vec::new()
    };
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
                let vc = value_contract_of_node(
                    &g,
                    &templates,
                    n,
                    &contracts_out,
                    &mut layouts_out,
                    &open_tm,
                    true,
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
                let toks = g.mods[0]
                    .imports
                    .get(nm)
                    .map(|&n| g.m.nodes[n as usize].pts.to_sorted())
                    .unwrap_or_default();
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
                .filter(|(nm, _)| !v.closed_refs.contains(nm))
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
    // A contract class a contract result or value names is published
    // whole (methods, statics): its readers call them on what they get.
    for l in layouts_out.iter_mut() {
        if let Some((_, c)) = contracts_out.iter().find(|(_, c)| c.display == l.display) {
            *l = c.clone();
        }
    }
    let shared_reads = if cfg.unit {
        g.m.shared_reads()
    } else {
        Default::default()
    };
    // What it does to the objects every unit shares (`model::SharedWrite`);
    // past the bound it is unknown code, which may write any key of them.
    let shared_out: Vec<model::SharedWrite> = if !cfg.unit {
        Vec::new()
    } else if g.m.aborted {
        use model::{Shared, SharedWrite};
        vec![
            SharedWrite::Store(Shared::Top, None),
            SharedWrite::Store(Shared::ObjectProto, None),
            SharedWrite::Store(Shared::BuiltinFn, None),
            SharedWrite::Unmodeled(Shared::ObjectProto),
            SharedWrite::Unmodeled(Shared::Builtin(model::BuiltinNs::GlobalThis)),
        ]
    } else {
        std::mem::take(&mut g.m.shared_out).into_iter().collect()
    };
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
        contracts_out,
        fn_contracts_out,
        fns_defined,
        layouts_out,
        values_out,
        args_out,
        ctor_args_out,
        reports,
        stores_out,
        writes_out,
        shared_out,
        shared_reads,
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
                ts.iter().all(|t| t.prefix_stable)
                    && ts
                        .iter()
                        .all(|t| format!("{:?}", t.fields) == format!("{:?}", ts[0].fields))
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
