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
    /// DECISIONS 210: per constructor function of another unit this unit
    /// constructs (contract name), what its `new`s pass.
    pub(crate) ctor_runs_out: Vec<(String, units::ArgContract)>,
    /// The displays in `layouts_out` that are other units' layouts (a
    /// stand-in's contract, forwarded): never over the owner's entry.
    pub(crate) layouts_fwd: Vec<String>,
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

#[path = "lib/contracts.rs"]
mod contracts;
use contracts::arg_contract_of;
use contracts::atoms_of;
use contracts::chain_unsettled;
use contracts::class_statics;
use contracts::closed_of;
use contracts::fn_inst_contract;
#[cfg(test)]
pub(crate) use contracts::cut_described_again;
use contracts::field_ta_contracts;
use contracts::field_value_contracts;
use contracts::fn_contract_name;
use contracts::fn_contract_of;
use contracts::foreign_parent;
use contracts::forwards_of;
use contracts::nested_layouts;
use contracts::open_templates;
use contracts::unnamed_methods;
use contracts::value_contract_of;
use contracts::value_contract_of_toks;

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
    g.m.per_unit = cfg.unit;
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
        g.published_ctors();
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
                        this_class: None,
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
                        fn_inst: false,
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
            // DECISIONS 210: every stand-in it calls, not only a method's:
            // a published function no unit runs is called by its owner's
            // code alone (`Gen::published_args`); a construction is no call
            // (`ctor_runs_out`).
            let mut v: Vec<String> = g
                .stand_in_fc
                .iter()
                .filter(|(&t, _)| g.m.called.contains(&g.m.tok_fn(t)))
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
    // DECISIONS 210: the constructor functions of other units this unit
    // constructs (contract names) and what its `new`s pass, as a published
    // method's calls publish theirs (DECISIONS 148).
    let ctor_runs_out: Vec<(String, units::ArgContract)> = match g.view {
        Some(_) if cfg.unit && !g.m.aborted => {
            let mut by: Vec<(String, Vec<model::Tok>)> = Vec::new();
            for (&t, fc) in &g.stand_in_fc {
                if !g.m.constructed.contains(&g.m.tok_fn(t)) {
                    continue;
                }
                match by.iter_mut().find(|(n, _)| *n == fc.name) {
                    Some((_, v)) => v.push(t),
                    None => by.push((fc.name.clone(), vec![t])),
                }
            }
            by.sort();
            by.into_iter()
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
                .collect()
        }
        _ => Vec::new(),
    };
    // DECISIONS 210: the layouts of this unit's constructor functions other
    // units construct (`Gen::published_ctors`, published in `contract_check`).
    if cfg.unit && !g.m.aborted {
        let mut tms: Vec<(model::FnIdx, model::TmplId)> = g
            .m
            .own_ctor_runs
            .iter()
            .filter_map(|&(f, _)| g.m.fn_inst.get(&f).map(|&tm| (f, tm)))
            .filter(|(_, tm)| g.m.fn_inst_published.contains(tm))
            .collect();
        tms.sort_unstable();
        tms.dedup();
        for (f, tm) in tms {
            let Some(c) = fn_inst_contract(&g, &templates, f, tm, &mut layouts_out, &open_tm)
            else {
                continue;
            };
            if !layouts_out.iter().any(|x| x.display == c.display) {
                layouts_out.push(c);
            }
        }
    }
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
    // DECISIONS 210: a construction of a function whose layout its owner
    // has not published yet read that layout (gone): this unit solves
    // again when the owner publishes it (`units::run`, DECISIONS 103).
    if cfg.unit {
        let mut pending: Vec<String> = g
            .stand_in_fc
            .iter()
            .filter(|(&t, _)| {
                let f = g.m.tok_fn(t);
                g.m.constructed.contains(&f) && matches!(g.m.foreign_ctor.get(&f), Some(None))
            })
            .map(|(_, fc)| format!("new {}", fc.name))
            .collect();
        pending.sort();
        pending.dedup();
        for d in pending {
            if !g.layouts_read.iter().any(|(x, _)| *x == d) {
                let seen = g.unit_in.map_or_else(
                    || units::digest(&None::<&units::ClassContract>),
                    |u| u.layout_seen(&d),
                );
                g.layouts_read.push((d, seen));
            }
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
        ctor_runs_out,
        layouts_fwd,
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
