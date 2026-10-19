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
pub mod delta;
pub mod derive;
mod dump;
mod gen;
mod gen_call;
mod gen_cjs;
mod gen_expr;
mod gen_const;
mod gen_rest;
pub mod model;
pub mod pts;
mod solve;
mod summary;
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
    /// Summary instances, the shared ones among them, and calls expanded
    /// into their caller's summary.
    pub instances: u64,
    pub shared_instances: u64,
    pub composed: u64,
    /// Solve work done (see `FactsConfig::max_work`).
    pub work: u64,
    /// The solve hit its budget: no facts were claimed.
    pub aborted: bool,
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
        let gt = g.m.tok(model::TokKind::Builtin(model::BuiltinNs::GlobalThis));
        builtins::note_native_write(&mut g.m, gt, None, "interpreted code may run");
    }
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
    if cfg.profile || cfg.explain {
        let mut kinds = [0usize; 7];
        for &(k, _, _) in g.m.sum.inst_key.keys() {
            kinds[k as usize] += 1;
        }
        eprintln!(
            "perry: wpfacts instances {} (call {} site {} shared {} entry {} group {}), program calls {}, calls {}",
            g.m.sum.insts.len(),
            kinds[0],
            kinds[6],
            kinds[5],
            kinds[2],
            kinds[3] + kinds[4],
            g.m.sum.n_prog,
            g.m.calls.len()
        );
    }
    if cfg.profile {
        // TEMP CW s7: kind-6 instances by callee and by source call.
        let mut by_f: std::collections::HashMap<u32, usize> = Default::default();
        let mut by_o: std::collections::HashMap<u32, usize> = Default::default();
        for &(k, o, f) in g.m.sum.inst_key.keys() {
            if k == 6 {
                *by_f.entry(f).or_default() += 1;
                *by_o.entry(o).or_default() += 1;
            }
        }
        let mut vf: Vec<_> = by_f.into_iter().collect();
        vf.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (f, n) in vf.iter().take(15) {
            eprintln!("perry: wpfacts TEMP k6 callee {n} {}", g.m.funcs[*f as usize].name);
        }
        let mut vo: Vec<_> = by_o.into_iter().collect();
        vo.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        eprintln!("perry: wpfacts TEMP k6 origins {} (prog {})", vo.len(), vo.iter().filter(|x| (x.0 as usize) < g.m.sum.n_prog).count());
        for (o, n) in vo.iter().take(15) {
            let owner = g.m.sum.owner.get(o).map(|f| g.m.funcs[*f as usize].name.clone()).unwrap_or_default();
            let c = &g.m.calls[*o as usize];
            let this = match c.this { model::ThisArg::Node(x) => format!("{}#{}", g.m.labels[x as usize], g.m.nodes[x as usize].pts.len()), _ => "-".into() };
            eprintln!("perry: wpfacts TEMP k6 origin {n} call {o} prog={} owner={owner} this={this}", (*o as usize) < g.m.sum.n_prog);
        }
        // TEMP CW s7: biggest instance interface terms, tokens vs templates.
        {
            let mut v: Vec<(usize, usize, String)> = Vec::new();
            let mut big = 0usize;
            let mut sum_inst = 0usize;
            for (i, inst) in g.m.sum.insts.iter().enumerate() {
                let fname = g.m.funcs[inst.f as usize].name.clone();
                let terms = inst.params.iter().enumerate().map(|(k, &p)| (format!("p{k}"), p)).chain([("this".to_string(), inst.this), ("ret".to_string(), inst.ret)]);
                for (what, n) in terms {
                    if n == model::NO_NODE || (n as usize) >= g.m.nodes.len() { continue; }
                    let toks = g.m.nodes[n as usize].pts.to_sorted();
                    sum_inst += toks.len();
                    if toks.len() > 64 { big += 1; }
                    let mut tms: std::collections::HashSet<u32> = Default::default();
                    for &t in &toks { if let Some(tm) = g.m.tmpl_of(t) { tms.insert(tm); } }
                    v.push((toks.len(), tms.len(), format!("inst{i}({}) {fname}.{what}", match inst.host { Some(h) => format!("host {h}"), None => "-".into() })));
                }
            }
            v.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
            eprintln!("perry: wpfacts TEMP instterms total_toks={sum_inst} terms>64={big} of {}", v.len());
            for (n, nt, w) in v.iter().take(4) {
                eprintln!("perry: wpfacts TEMP instterm {n} toks {nt} tmpls {w}");
            }
            // Feeders of the biggest term: preds with label and size.
            let mut best: Option<(usize, u32)> = None;
            for inst in g.m.sum.insts.iter() {
                for &p in inst.params.iter() {
                    if p == model::NO_NODE || (p as usize) >= g.m.nodes.len() { continue; }
                    let l = g.m.nodes[p as usize].pts.len();
                    if best.map_or(true, |b| l > b.0) { best = Some((l, p)); }
                }
            }
            if let Some((_, x)) = best {
                // Calls whose argument edges reach the biggest term directly.
                let owner = g.m.node_fn[x as usize];
                eprintln!("perry: wpfacts TEMP bigterm {}#{} of {}", g.m.labels[x as usize], g.m.nodes[x as usize].pts.len(), if owner == model::NO_FN { "-".to_string() } else { g.m.funcs[owner as usize].name.clone() });
                let mut shown = 0;
                for (ci, c) in g.m.calls.iter().enumerate() {
                    if shown >= 3 { break; }
                    if c.args.iter().any(|&a| g.m.nodes[a as usize].succ.iter().any(|&b| b == x)) {
                        shown += 1;
                        let this = match c.this { model::ThisArg::Node(n) => {
                            let toks = g.m.nodes[n as usize].pts.to_sorted();
                            let mut tms: std::collections::HashSet<u32> = Default::default();
                            for &t in &toks { if let Some(tm) = g.m.tmpl_of(t) { tms.insert(tm); } }
                            let nf = g.m.node_fn[n as usize];
                            format!("{}#{} tmpls {} top={} in {}", g.m.labels[n as usize], toks.len(), tms.len(), toks.contains(&model::TOP), if nf == model::NO_FN { "-".to_string() } else { g.m.funcs[nf as usize].name.clone() })
                        }, _ => "-".into() };
                        for (h, node) in g.m.nodes.iter().enumerate() {
                            for c2 in node.cons.iter() {
                                let names = match c2 { model::Cons::Call { call } | model::Cons::MCall { call, .. } | model::Cons::CallThis { call, .. } | model::Cons::CallArgs { call } => *call == ci as u32, _ => false };
                                if names {
                                    let toks = node.pts.to_sorted();
                                    let nf = g.m.node_fn[h];
                                    let funcs: Vec<String> = toks.iter().take(4).map(|&t| g.m.tok_name(t)).collect();
                                    eprintln!("perry: wpfacts TEMP bigcallee {ci} {} {}#{} in {} e.g. {:?}", c2.kind_name(), g.m.labels[h], toks.len(), if nf == model::NO_FN { "-".to_string() } else { g.m.funcs[nf as usize].name.clone() }, funcs);
                                }
                            }
                        }
                        eprintln!("perry: wpfacts TEMP bigcall {ci} prog={} tmpl={:?} site={} this={this}", ci < g.m.sum.n_prog, g.m.sum.tmpl_of.get(&(ci as u32)), c.site);
                    }
                }
                let mut stack = vec![(x, 0u32)];
                let mut seen: std::collections::HashSet<u32> = Default::default();
                while let Some((y, depth)) = stack.pop() {
                    if depth > 4 || !seen.insert(y) { continue; }
                    for (a, node) in g.m.nodes.iter().enumerate() {
                        if node.succ.iter().any(|&b| b == y) {
                            let fname = { let f = g.m.node_fn[a]; if f == model::NO_FN { "-".to_string() } else { g.m.funcs[f as usize].name.clone() } };
                            eprintln!("perry: wpfacts TEMP feed d{depth} {}#{} <- {}#{} ({}) obs={}", g.m.labels[y as usize], y, g.m.labels[a], g.m.nodes[a].pts.len(), fname, node.obs);
                            if seen.len() < 40 { stack.push((a as u32, depth + 1)); }
                        }
                    }
                }
            }
            let mut site = 0usize;
            for k in g.m.toks.iter() { if matches!(k, model::TokKind::Obj(_, c) if *c != model::NO_CTX) { site += 1; } }
            eprintln!("perry: wpfacts TEMP tokens {} site-obj {} ctx-ids {}", g.m.toks.len(), site, g.m.sum.ctx_ids.len());
        }
        let mut vw: Vec<_> = g.m.sum.temp_why.iter().collect();
        vw.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (w, n) in vw.iter().take(12) {
            eprintln!("perry: wpfacts TEMP factory-why {n} {w}");
        }
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
        shared_instances: g.m.sum.inst_key.keys().filter(|k| k.0 == 5).count() as u64,
        composed: g.m.sum.composed.len() as u64,
        work: g.m.work,
        aborted: g.m.aborted,
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
                    model::TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
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
