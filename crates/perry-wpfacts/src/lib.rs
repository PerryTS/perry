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
pub mod derive;
mod dump;
mod gen;
mod gen_call;
mod gen_expr;
mod gen_rest;
pub mod model;
pub mod pts;
mod solve;
#[cfg(test)]
mod tests;

pub use bindings::{BindingFacts, ModuleBindings};
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
    /// Report-only what-if: ignore the E14 program-wide kill (UNSOUND; for
    /// the precision report only, never for codegen).
    pub whatif_no_e14: bool,
    /// Per-constraint-kind time profile in the report.
    pub profile: bool,
    /// Bound on points-to entries (work/memory); past it, no facts.
    pub max_pts: Option<u64>,
    /// Deterministic solve-work bound (token propagations along edges and
    /// into constraints); past it, no facts. Unlike the wall budget it
    /// gives the same answer on every machine.
    pub max_work: Option<u64>,
    /// Widening bound on one points-to set (escape + ⊤ beyond it).
    pub widen_k: Option<usize>,
    /// The driver knows the program can run interpreted code (a deferred
    /// `new Function`/`eval` site or `node:vm`): E14, program-wide.
    pub dynamic_code: Option<String>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    /// Diagnostics: name where ⊤ came from in each ⊤ site verdict.
    pub explain: bool,
}

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
    /// Solve work done (see `FactsConfig::max_work`).
    pub work: u64,
    /// The solve hit its budget: no facts were claimed.
    pub aborted: bool,
    pub top_calls: u64,
    /// (constraint kind, applications, ms), most expensive first.
    pub profile: Vec<(String, u64, f64)>,
    pub pts_entries: u64,
    pub widened_nodes: u64,
    /// The largest points-to sets: (node label, size, token-kind mix).
    pub big_nodes: Vec<(String, usize, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct ProgramFacts {
    pub module_paths: Vec<String>,
    pub templates: Vec<TemplateFacts>,
    pub sites: Vec<SiteFacts>,
    pub stats: Stats,
    /// A program-wide kill (E14 interpreted code), if any.
    pub global: Option<String>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    pub sabotage: Option<String>,
    /// Report-only what-if run (unsound): never feed codegen from it.
    pub whatif_no_e14: bool,
    pub relations: Option<Vec<String>>,
    pub tok_names: Vec<String>,
    /// Receiver facts per binding, per module (slice 2: codegen elision).
    pub bindings: Vec<ModuleBindings>,
    module_digests: Vec<u64>,
}

/// Run the analysis over every module (paths must be the keys the caller
/// uses for `digest_for` / `contract_section`).
pub fn compute(modules: &[(String, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let t0 = std::time::Instant::now();
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut g = gen::Gen::new(sorted.clone(), cfg.relations, cfg.explain);
    let interp = cfg.dynamic_code.clone().unwrap_or_else(|| "-".into());
    let dynamic_reach = cfg.dynamic_code_reach.globals as u8;
    g.m.rel(|| format!("dynamicreach {dynamic_reach}"));
    g.m.rel(|| format!("interp {}", interp.replace(' ', "\\s")));
    if cfg.whatif_no_e14 {
        g.m.rel(|| "whatif 1".to_string());
    }
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
    g.m.widen_k = cfg.widen_k.unwrap_or(0);
    g.register_all();
    g.link_exports();
    g.walk_all();
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
    g.m.solve();
    g.m.dump_meta();
    let solve_ms = t1.elapsed().as_secs_f64() * 1000.0;
    // E14: with the interpreter linked, interpreted code can patch
    // Object.prototype; it is reachable through a modeled Function/vm use or
    // any call of a ⊤ value (which could be the Function constructor).
    if let Some(linked) = &cfg.dynamic_code {
        if !cfg.whatif_no_e14 && (g.m.dyn_eval.is_some() || g.m.top_calls > 0) {
            g.global_hard = Some(format!(
                "E14 interpreter linked ({linked}) and reachable ({})",
                g.m.dyn_eval.clone().unwrap_or_else(|| format!(
                    "{} calls of unknown function values",
                    g.m.top_calls
                ))
            ));
        }
    }
    // Source the interpreter evaluates can name the global scope, so it can
    // install an accessor under any key on Object.prototype: every family's
    // chain runs unknown code with the receiver (the E23 rule, for any key).
    if cfg.dynamic_code_reach.globals && !cfg.whatif_no_e14 {
        let first = cfg
            .dynamic_code_reach
            .sites
            .iter()
            .next()
            .cloned()
            .unwrap_or_default();
        g.global_hard = Some(format!(
            "E14 dynamic_code_reach: non-constant source names Object.prototype ({first})"
        ));
    }
    let t2 = std::time::Instant::now();
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
    let global = g.global_hard.clone();
    if let Some(spec) = &cfg.sabotage {
        dump::apply_sabotage(&mut templates, spec);
    }
    let derive_ms = t2.elapsed().as_secs_f64() * 1000.0;
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
        work: g.m.work,
        aborted: g.m.aborted,
        pts_entries: g.m.pts_entries,
        widened_nodes: g.m.widened_nodes,
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
    let mut pf = ProgramFacts {
        module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
        templates,
        sites,
        stats,
        global,
        dynamic_code_reach: cfg.dynamic_code_reach.clone(),
        sabotage: cfg.sabotage.clone(),
        whatif_no_e14: cfg.whatif_no_e14,
        relations: g.m.relations.take(),
        tok_names,
        bindings,
        module_digests: Vec::new(),
    };
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
                    model::TokKind::Obj(tm) => match m.tmpls[*tm as usize].kind {
                        model::TmplKind::Class { .. } => "class",
                        model::TmplKind::Lit { .. } => "lit",
                        model::TmplKind::FnInst { .. } => "fninst",
                        model::TmplKind::Arr { .. } => "arr",
                        _ => "bobj",
                    },
                    model::TokKind::Func(_) => "func",
                    model::TokKind::Bound(_) => "bound",
                    model::TokKind::ClassObj(_) => "classobj",
                    model::TokKind::Proto(_) => "proto",
                    _ => "builtin",
                };
                *mix.entry(k).or_default() += 1;
            }
            let label = m.labels.get(i).copied().unwrap_or("?");
            (
                format!("{label}#{i}"),
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
