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
mod cwl_census;
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
mod units;
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
    /// The driver's resolution of every `export … from "src"` source: a
    /// re-export has no import of its source to resolve it by.
    pub export_sources: ExportSources,
    /// The modules are one unit of a larger program: every other unit is
    /// unknown compiled code (`Gen::unit_boundary`).
    pub unit: bool,
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
    /// Per-unit mode: each unit's path and statistics.
    pub units: Vec<(String, Stats)>,
    /// Per-unit mode: driver rounds (1 + census re-solves) and the classes
    /// the census broke (design §11.3).
    pub rounds: u32,
    pub broken: Vec<String>,
    /// Solve work of every unit, per round (`stats.work` is their sum).
    pub round_work: Vec<u64>,
    /// One unit's class contracts and census reports (design §11).
    pub(crate) contracts_out: Vec<((String, String), units::ClassContract)>,
    pub(crate) fn_contracts_out: Vec<((String, String), units::FnContract)>,
    pub(crate) layouts_out: Vec<units::ClassContract>,
    pub(crate) reports: Vec<String>,
    module_digests: Vec<u64>,
}

/// Run the analysis over every module (paths must be the keys the caller
/// uses for `digest_for` / `contract_section`).
pub fn compute(modules: &[(String, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    compute_in(modules, cfg, None)
}

/// [`compute`] for one unit of a per-unit run: `published` is what the other
/// units can name of its modules (`units::published_surface`).
pub(crate) fn compute_in<'a>(
    modules: &'a [(String, &'a Module)],
    cfg: &FactsConfig,
    published: Option<&'a units::Published>,
) -> ProgramFacts {
    let t0 = std::time::Instant::now();
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut g = gen::Gen::new(sorted.clone(), cfg.relations, cfg.explain);
    g.export_sources = cfg.export_sources.clone();
    g.unit_in = published;
    if cfg.profile { // TEMP CW9: module kinds for the cross-module event recorder (1 = benchmark, 2 = shared)
        g.m.cw9_modkind = sorted.iter().map(|(p, _)| if ["harness.js", "run.js", "benchmark.js", "som.js", "webpack.config.js"].iter().any(|s| p.ends_with(s)) { 2u8 } else { 1u8 }).collect();
    }
    let interp = cfg.dynamic_code.clone().unwrap_or_else(|| "-".into());
    let dynamic_reach = cfg.dynamic_code_reach.globals as u8;
    g.m.rel(|| format!("dynamicreach {dynamic_reach}"));
    g.m.rel(|| format!("interp {}", interp.replace(' ', "\\s")));
    if cfg.profile {
        g.m.profile = Some(Default::default());
        g.m.cwl_trace = std::env::var("CWL_TRACE").ok().and_then(|v| v.parse().ok()); // TEMP CWL
        g.m.cwl_tok = std::env::var("CWL_TOK").ok().and_then(|v| v.parse().ok()); // TEMP CWL
        g.m.cwl_inst_probe = std::env::var_os("CWL_INST").is_some(); // TEMP CWL
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
    if cfg.unit {
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
    while cfg.unit && g.contract_check() {
        g.m.solve();
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
    // TEMP CWL census.
    if cfg.profile && sorted.len() == 1 && std::env::var("CWL_CENSUS").is_ok_and(|f| sorted[0].0.contains(f.as_str())) {
        cwl_census::run(&g.m, sorted[0].0);
    }
    // TEMP CW s8 debug: every node with tokens.
    if std::env::var_os("CW_DBG").is_some() {
        let m = &g.m;
        for (n, node) in m.nodes.iter().enumerate() {
            if node.pts.is_empty() { continue; }
            let f = m.node_fn[n];
            let names: Vec<String> = node.pts.to_sorted().iter().map(|&t| format!("{}{}", m.tok_name(t), match m.toks[t as usize] { model::TokKind::Obj(_, c) | model::TokKind::Func(_, c) if c != model::NO_CTX => format!("@{c}"), _ => String::new() })).collect();
            let succ: Vec<u32> = node.succ.iter().copied().collect();
            eprintln!("DBG {}#{n} fn={} obs={} pts={:?} succ={:?} cons={:?}", m.labels[n], if f == model::NO_FN { "-".into() } else { m.funcs[f as usize].name.clone() }, node.obs, names, succ, node.cons.iter().map(|c| c.kind_name()).collect::<Vec<_>>());
        }
        eprintln!("DBG stats {}", m.sum.stats);
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
    // TEMP CW s9 diagnosis: biggest field nodes and one token's provenance.
    if cfg.profile {
        let m = &g.m;
        let mut fnodes: Vec<(u32, String, String)> = Vec::new();
        for (&(t, a), &n) in &m.fields { fnodes.push((n, m.tok_name(t) + &format!("#{t}"), m.key_name(model::Key::Named(a)))); }
        for (&t, &n) in &m.field_any { fnodes.push((n, m.tok_name(t) + &format!("#{t}"), "<any>".into())); }
        for (&t, &n) in &m.all_fields { fnodes.push((n, m.tok_name(t) + &format!("#{t}"), "<all>".into())); }
        let owner_of: crate::pts::FxHashMap<u32, String> = fnodes.iter().map(|(n, t, k)| (*n, format!("[{t}.{k}]"))).collect();
        let desc = |n: u32| -> String {
            let node = &m.nodes[n as usize];
            let own = owner_of.get(&n).cloned().unwrap_or_default();
            let toks = node.pts.to_sorted();
            let mut tm = crate::pts::FxHashSet::default();
            let (mut site, mut plain, mut other) = (0, 0, 0);
            for &t in toks.iter() { match m.toks[t as usize] { model::TokKind::Obj(x, c) => { tm.insert(x); if c == model::NO_CTX { plain += 1 } else { site += 1 } } _ => other += 1 } }
            let f = m.node_fn[n as usize];
            let (f, kk) = match m.cw9_inst_fn.get(&n) { Some(&(f, k)) => (f, format!("k{}/{}", k >> 28, k & 0xfffffff)), None => (f, String::new()) };
            format!("{}{kk}#{n}{own} fn={} obs={} toks={} tmpls={} site={site} plain={plain} other={other} succ={} cons={} work={}", m.labels[n as usize], if f == model::NO_FN { "-".into() } else { m.funcs[f as usize].name.clone() }, node.obs, toks.len(), tm.len(), node.succ.len(), node.cons.len(), m.cw9_node_work.get(&n).copied().unwrap_or(0))
        };
        let mut by_work = fnodes.clone();
        by_work.sort_by_key(|x| std::cmp::Reverse(m.cw9_node_work.get(&x.0).copied().unwrap_or(0)));
        for (n, t, k) in by_work.iter().take(20) { eprintln!("CW9 fieldwork owner={t} key={k} {}", desc(*n)); }
        fnodes.sort_by_key(|x| std::cmp::Reverse(m.nodes[x.0 as usize].pts.len()));
        for (n, t, k) in fnodes.iter().take(20) { eprintln!("CW9 fieldpts owner={t} key={k} {}", desc(*n)); }
        let mut all: Vec<(u64, u32)> = m.cw9_node_work.iter().map(|(&n, &w)| (w, n)).collect();
        all.sort_unstable_by(|a, b| b.cmp(a));
        for (_, n) in all.iter().take(15) { eprintln!("CW9 nodework {}", desc(*n)); }
        if let Some(&(root, _, _)) = by_work.first() {
            for (&(t, _a), &n) in &m.fields { if n == root {
                if let model::TokKind::Obj(tm, c) = m.toks[t as usize] { eprintln!("CW9 owner tmpl {:?} ctx {c}", m.tmpls[tm as usize].kind); if let model::TmplKind::BObj { site, .. } | model::TmplKind::Arr { site, .. } = m.tmpls[tm as usize].kind { if (site as usize) < m.stores.len() { let st = &m.stores[site as usize]; eprintln!("CW9 owner site {site} desc={} func={}", st.desc, if st.func == u32::MAX { "-".into() } else { m.funcs[st.func as usize].name.clone() }); } if c != model::NO_CTX && (c as usize) < m.calls.len() { let cs = &m.calls[c as usize]; eprintln!("CW9 owner call args={:?}", cs.args.iter().map(|&a| desc(a)).collect::<Vec<_>>()); } } }
                for (i, node) in m.nodes.iter().enumerate() { if node.succ.contains(&root) { eprintln!("CW9 rootpred {} cons={:?}", desc(i as u32), node.cons.iter().take(6).map(|c| c.kind_name()).collect::<Vec<_>>()); } }
                let mut nh = 0;
                for (i, node) in m.nodes.iter().enumerate() {
                    if nh >= 8 || !node.pts.contains_tok(t) { continue; }
                    for c in &node.cons { if let model::Cons::Store { key, src, site } = c { if *key == model::Key::Named(_a) {
                        nh += 1;
                        let mut kinds: std::collections::BTreeMap<&str, u32> = Default::default();
                        for x in node.pts.to_sorted() { *kinds.entry(match m.toks[x as usize] { model::TokKind::Obj(..) => "obj", model::TokKind::Func(..) => "fn", model::TokKind::Proto(_) => "proto", model::TokKind::ClassObj(_) => "class", _ => "other" }).or_default() += 1; }
                        let st = &m.stores[*site as usize];
                        eprintln!("CW9 storeholder {} kinds={kinds:?} site={} in {} src={}", desc(i as u32), st.desc, if st.func == u32::MAX { "-".into() } else { m.funcs[st.func as usize].name.clone() }, desc(*src));
                    } } }
                }
                let any = m.field_any[&t];
                for (i, node) in m.nodes.iter().enumerate() { if node.succ.contains(&any) { eprintln!("CW9 anypred {} cons={:?}", desc(i as u32), node.cons.iter().take(6).map(|c| c.kind_name()).collect::<Vec<_>>()); } }
            } }
        }
        {
            let mut h: crate::pts::FxHashMap<(u32, u32), (u32, u64)> = Default::default();
            for (&n, &(f, k)) in &m.cw9_inst_fn { let l = m.nodes[n as usize].pts.len(); if l > 500 { let e = h.entry((k >> 28, f)).or_default(); e.0 += 1; e.1 += m.cw9_node_work.get(&n).copied().unwrap_or(0); } }
            let mut v: Vec<_> = h.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1 .1));
            for ((k, f), (c, w)) in v.iter().take(25) { eprintln!("CW9 biginst kind={k} fn={} nodes={c} work={w}", m.funcs[*f as usize].name); }
        }
        {
            let mut v: Vec<_> = m.cw9_shared.iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(*x.1));
            for ((tag, f, why, tf), c) in v.iter().take(25) { eprintln!("CW9 shared tag={tag} fn={} why={} from={} n={c}", m.funcs[*f as usize].name, ["", "in-scc", "no-origin-tmpl", "no-origin-notmpl"][*why as usize], if *tf == u32::MAX { "-".into() } else { m.funcs[*tf as usize].name.clone() }); }
        }
        for &(p, a, b, t) in &m.cw9_events { eprintln!("CW9 event pop={p} tok={}#{t} from {} to {} cons_from={:?}", m.tok_name(t), desc(a), desc(b), m.nodes[a as usize].cons.iter().take(5).map(|c| c.kind_name()).collect::<Vec<_>>()); }
        // Provenance of one site token (the median one) in the heaviest field node.
        if let Some(&(root, _, _)) = by_work.first() {
            let mut preds: Vec<Vec<u32>> = vec![Vec::new(); m.nodes.len()];
            for (i, node) in m.nodes.iter().enumerate() { for &s in &node.succ { preds[s as usize].push(i as u32); } }
            let toks = m.nodes[root as usize].pts.to_sorted();
            let objs: Vec<u32> = toks.iter().copied().filter(|&t| matches!(m.toks[t as usize], model::TokKind::Obj(..))).collect();
            let mut jobs: Vec<(u32, u32)> = [objs.first(), objs.get(objs.len() / 2)].into_iter().flatten().map(|&t| (root, t)).collect();
            if let Ok(v) = std::env::var("CW9_TRACE") {
                for j in v.split(',') { let mut it = j.split(':'); if let (Some(a), Some(b)) = (it.next(), it.next()) { if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) { jobs.push((a, b)); } } }
            }
            // A node by label prefix: print the members of it by kind.
            if let Ok(v) = std::env::var("CW9_NODE") {
                for j in v.split(',') { if let Ok(n) = j.parse::<u32>() { let names: Vec<String> = m.nodes[n as usize].pts.to_sorted().iter().filter(|&&t| !matches!(m.toks[t as usize], model::TokKind::Obj(..))).take(40).map(|&t| format!("{}#{t}", m.tok_name(t))).collect(); eprintln!("CW9 node {} nonobj={names:?}", desc(n)); } }
            }
            if let Ok(v) = std::env::var("CW9_TMPL") {
                for j in v.split(',') { if let Ok(tm) = j.parse::<usize>() { let k = &m.tmpls[tm].kind; let st = String::new();
                    eprintln!("CW9 tmpl {tm} {} {k:?} {st} tmpl_fn={:?} obs_tok={}", m.tmpls[tm].display, m.tmpl_fn.get(&(tm as u32)).map(|&f| m.funcs[f as usize].name.clone()), m.tok_index.get(&model::TokKind::Obj(tm as u32, model::NO_CTX)).is_some_and(|t| m.obs_toks.contains(t))); } }
            }
            if let Ok(v) = std::env::var("CW9_HOLD") {
                for j in v.split(',') { if let Ok(t) = j.parse::<u32>() { let mut c = 0; for (i, node) in m.nodes.iter().enumerate() { if node.pts.contains_tok(t) { c += 1; if c <= 12 || m.node_fn[i] != model::NO_FN { if c <= 40 { eprintln!("CW9 hold {t} {} rel={}", desc(i as u32), m.labels[i]); } } } } eprintln!("CW9 hold {t} total {c}"); } }
            }
            if let Ok(v) = std::env::var("CW9_SH") {
                for j in v.split(',') { if let Ok(t) = j.parse::<u32>() { let mut c = 0;
                    for (i, node) in m.nodes.iter().enumerate() { if node.obs || !node.pts.contains_tok(t) { continue; } for cs in &node.cons { if let model::Cons::Store { key, src, site } = cs { c += 1; if c > 25 { continue; } let st = &m.stores[*site as usize];
                        eprintln!("CW9 sh {t} key={} holder={} site={} in {} src={}", m.key_name(*key), desc(i as u32), st.desc, if st.func == u32::MAX { "-".into() } else { m.funcs[st.func as usize].name.clone() }, desc(*src)); } } }
                    eprintln!("CW9 sh {t} total {c}"); } }
            }
            if let Ok(v) = std::env::var("CW9_DST") {
                for j in v.split(',') { let pat = format!(": {j} "); let pat2 = format!(": {j},"); let pat3 = format!(": {j} }}");
                    for (i, node) in m.nodes.iter().enumerate() { for c in &node.cons { let d = format!("{c:?}"); if d.contains(&pat) || d.contains(&pat2) || d.contains(&pat3) { eprintln!("CW9 dst {j} from {} cons={d}", desc(i as u32)); } } } }
            }
            for (root, tok) in jobs {
                eprintln!("CW9 trace tok {}#{tok} {:?}", m.tok_name(tok), m.toks[tok as usize]);
                let mut prev: crate::pts::FxHashMap<u32, u32> = Default::default();
                let mut q = std::collections::VecDeque::from([root]);
                prev.insert(root, u32::MAX);
                let mut last = root;
                let mut found = false;
                while let Some(x) = q.pop_front() {
                    if !found { last = x; }
                    if !found && !preds[x as usize].iter().any(|&p| m.nodes[p as usize].pts.contains_tok(tok)) { found = true; }
                    for &p in &preds[x as usize] { if !prev.contains_key(&p) && m.nodes[p as usize].pts.contains_tok(tok) { prev.insert(p, x); q.push_back(p); } }
                }
                // `last` = a farthest origin; print the chain origin -> root.
                let mut x = last; let mut hops = 0;
                loop {
                    let node = &m.nodes[x as usize];
                    eprintln!("CW9   hop {} cons={:?}", desc(x), node.cons.iter().take(4).map(|c| c.kind_name()).collect::<Vec<_>>());
                    hops += 1;
                    let nx = prev[&x]; if nx == u32::MAX || hops > 60 { break; } x = nx;
                }
                let origins = prev.keys().filter(|&&p| !preds[p as usize].iter().any(|&q| m.nodes[q as usize].pts.contains_tok(tok))).count();
                eprintln!("CW9   reached {} nodes, {} origins", prev.len(), origins);
            }
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
        shared_instances: g.m.sum.inst_key.keys().filter(|k| k.0 == 1 && k.1 == summary::LAY_NONE).count() as u64,
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
    // Per-unit run: the classes this unit publishes as contracts, and what
    // it did beyond other units' contracts (design §11.2, §11.3).
    let reports = if cfg.unit { g.reports() } else { Vec::new() };
    let contracts_out: Vec<((String, String), units::ClassContract)> = if cfg.unit && !g.m.aborted {
        g.class_contracts
            .iter()
            .filter_map(|&((mi, a), _, tm)| {
                let facts = templates.iter().find(|t| t.id == tm)?.clone();
                let model::TmplKind::Class { name, class_id, literal_shape, .. } = g.m.tmpls[tm as usize].kind.clone() else {
                    return None;
                };
                let mut methods = Vec::new();
                let mut native_parent = None;
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
                            (ts.len() == 1 && f != model::NO_FN)
                                .then(|| (g.m.atoms[k as usize].clone(), g.m.funcs[f as usize].name.clone()))
                        })
                        .filter(|(k, _)| !methods.iter().any(|(m, _): &(String, String)| m == k))
                        .collect();
                    own.sort();
                    methods.extend(own);
                    native_parent = g.m.tmpls[c as usize].native_parent.clone();
                    cur = g.m.tmpls[c as usize].parent;
                }
                let key = (sorted[mi as usize].0.to_string(), g.m.atoms[a as usize].clone());
                Some((key, units::ClassContract {
                    display: g.m.tmpls[tm as usize].display.clone(),
                    name,
                    class_id,
                    literal_shape,
                    facts,
                    methods,
                    native_parent,
                }))
            })
            .collect()
    } else {
        Vec::new()
    };
    // Its function contracts and the layouts they return (design §11.2).
    let mut layouts_out: Vec<units::ClassContract> = Vec::new();
    let fn_contracts_out: Vec<((String, String), units::FnContract)> = if cfg.unit && !g.m.aborted {
        let mut out = Vec::new();
        for &((mi, a), f) in &g.fn_contracts {
            let fi = &g.m.funcs[f as usize];
            let params = (0..fi.params.len() as u32)
                .map(|i| g.m.marker_of.get(&(f, i)).and_then(|t| g.m.marker_use.get(t)).copied().unwrap_or(0))
                .collect();
            let rets: Vec<model::NodeId> = g.m.contract_rets.iter().filter(|(g2, _)| *g2 == f).map(|&(_, n)| n).collect();
            let ret = (!g.ret_top.contains(&f) && !rets.is_empty()).then(|| {
                let mut ds: Vec<String> = Vec::new();
                for n in rets {
                    for t in g.m.nodes[n as usize].pts.to_sorted() {
                        let Some(tm) = g.m.tmpl_of(t) else { continue };
                        let display = g.m.tmpls[tm as usize].display.clone();
                        if ds.contains(&display) {
                            continue;
                        }
                        if let (Some(facts), model::TmplKind::Class { name, class_id, literal_shape, .. }) =
                            (templates.iter().find(|x| x.id == tm), g.m.tmpls[tm as usize].kind.clone())
                        {
                            if !layouts_out.iter().any(|c| c.display == display) {
                                layouts_out.push(units::ClassContract {
                                    display: display.clone(),
                                    name,
                                    class_id,
                                    literal_shape,
                                    facts: facts.clone(),
                                    methods: Vec::new(),
                                    native_parent: None,
                                });
                            }
                        }
                        ds.push(display);
                    }
                }
                ds.sort();
                ds
            });
            let key = (sorted[mi as usize].0.to_string(), g.m.atoms[a as usize].clone());
            out.push((key, units::FnContract {
                name: fi.name.clone(),
                params,
                variadic: fi.rest.is_some() || fi.has_arguments,
                ret,
            }));
        }
        out
    } else {
        Vec::new()
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
        rounds: 0,
        broken: Vec::new(),
        round_work: Vec::new(),
        contracts_out,
        fn_contracts_out,
        layouts_out,
        reports,
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
                    && ts.iter().all(|t| format!("{:?}", t.fields) == format!("{:?}", ts[0].fields))
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
