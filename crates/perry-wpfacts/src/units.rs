//! Per-unit driver pieces (decision 151, design §11): what a unit
//! publishes to the other units of the program.

use crate::pts::{FxHashMap, FxHashSet};
use crate::{FactsConfig, ProgramFacts, SiteFacts, TemplateFacts};
use perry_hir::{Export, ImportSpecifier, Module};

/// A published class as its importers see it (design §11.2): its template
/// and the facts its defining unit proved, its prototype methods (key,
/// function name) along the chain inside that unit, and its native parent.
#[derive(Clone, Debug)]
pub(crate) struct ClassContract {
    pub display: String,
    pub name: String,
    pub class_id: u32,
    pub literal_shape: bool,
    pub facts: TemplateFacts,
    pub methods: Vec<(String, String)>,
    pub native_parent: Option<(String, String)>,
}

/// A published function as its callers see it (design §11.2): per
/// parameter what the callee does with everything reachable from the
/// argument (`model::MARK_*`: read, touched by a protocol, or used beyond:
/// then it escapes), whether extra arguments reach it (`variadic`: all
/// arguments escape), and the layouts it returns (`None`: ⊤).
#[derive(Clone, Debug)]
pub(crate) struct FnContract {
    pub name: String,
    pub params: Vec<u8>,
    pub variadic: bool,
    pub ret: Option<Vec<String>>,
}

/// What the other units can name of each module (§11.1): the export names
/// some module imports or re-exports by name, or the whole namespace where
/// a module imports it as a namespace, re-exports all of it, or a dynamic
/// `import()` can reach it. Syntactic and linear in the import edges.
#[derive(Clone, Debug, Default)]
pub(crate) struct Published {
    whole: FxHashSet<String>,
    names: FxHashMap<String, FxHashSet<String>>,
    /// Interpreted code can import any module: everything is published.
    all: bool,
    /// Templates (by display) the census found used beyond their contract:
    /// published as escapes.
    pub(crate) broken: FxHashSet<String>,
    /// The class contracts of the units solved so far, by (defining module
    /// path, export name).
    pub(crate) classes: FxHashMap<(String, String), ClassContract>,
    /// Their function contracts, by (defining module path, export name).
    pub(crate) fns: FxHashMap<(String, String), FnContract>,
    /// The layouts contract functions return, by template display.
    pub(crate) layouts: FxHashMap<String, ClassContract>,
}

impl Published {
    pub(crate) fn whole(&self, path: &str) -> bool {
        self.all || self.whole.contains(path)
    }

    pub(crate) fn fn_contract(&self, path: &str, name: &str) -> Option<&FnContract> {
        self.fns.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn layout(&self, display: &str) -> Option<&ClassContract> {
        self.layouts.get(display)
    }

    pub(crate) fn class_contract(&self, path: &str, name: &str) -> Option<&ClassContract> {
        self.classes.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn broken(&self, display: &str) -> bool {
        self.broken.contains(display)
    }

    pub(crate) fn names(&self, path: &str, name: &str) -> bool {
        self.whole(path) || self.names.get(path).is_some_and(|n| n.contains(name))
    }
}

pub(crate) fn published_surface(
    sorted: &[(&str, &Module)],
    cfg: &FactsConfig,
) -> Published {
    let mut p = Published {
        all: cfg.dynamic_code.is_some()
            || cfg.dynamic_code_reach.globals
            || !cfg.dynamic_code_reach.sites.is_empty(),
        ..Default::default()
    };
    for (path, module) in sorted {
        let resolve = |source: &str| {
            module
                .imports
                .iter()
                .find(|i| i.source == source)
                .and_then(|i| i.resolved_path.clone())
        };
        for imp in &module.imports {
            let Some(t) = imp.resolved_path.as_ref().filter(|t| t.as_str() != *path) else {
                continue;
            };
            if imp.type_only || imp.runtime_erased {
                continue;
            }
            if imp.is_dynamic {
                p.whole.insert(t.clone());
            }
            for s in &imp.specifiers {
                match s {
                    ImportSpecifier::Named { imported, .. } => {
                        p.names.entry(t.clone()).or_default().insert(imported.clone());
                    }
                    ImportSpecifier::Default { .. } => {
                        p.names.entry(t.clone()).or_default().insert("default".into());
                    }
                    ImportSpecifier::Namespace { .. } => {
                        p.whole.insert(t.clone());
                    }
                }
            }
        }
        for e in &module.exports {
            match e {
                Export::ReExport { source, imported, .. } => {
                    if let Some(t) = resolve(source) {
                        p.names.entry(t).or_default().insert(imported.clone());
                    }
                }
                Export::ExportAll { source } | Export::NamespaceReExport { source, .. } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t);
                    }
                }
                Export::Named { .. } => {}
            }
        }
    }
    p
}

/// Modules in dependency order (an imported module before its importers),
/// so a contract is computed before a unit consumes it. In an import cycle
/// the back edge has no contract yet: that import is ⊤, as in P1.
fn dependency_order(sorted: &[(&str, &Module)]) -> Vec<usize> {
    let index: FxHashMap<&str, usize> = sorted.iter().enumerate().map(|(i, (p, _))| (*p, i)).collect();
    let mut state = vec![0u8; sorted.len()];
    let mut out = Vec::with_capacity(sorted.len());
    for root in 0..sorted.len() {
        if state[root] != 0 {
            continue;
        }
        // Iterative DFS, post-order.
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            let imports = &sorted[v].1.imports;
            if *next < imports.len() {
                let i = *next;
                *next += 1;
                let w = imports[i].resolved_path.as_deref().and_then(|p| index.get(p).copied());
                if let Some(w) = w.filter(|&w| state[w] == 0) {
                    state[w] = 1;
                    stack.push((w, 0));
                }
            } else {
                state[v] = 2;
                out.push(v);
                stack.pop();
            }
        }
    }
    out
}

/// The per-unit driver (decision 151, design §7 and §11): one solve per
/// module in dependency order, each publishing contracts to the units after
/// it; then the census. A class some unit used beyond its contract is
/// broken: it is published as an escape, and the units that see it again
/// (its defining unit and every unit importing that one, transitively:
/// contracts travel only along import edges) are solved again; every other
/// unit's inputs are unchanged, so its result is kept. Broken only grows,
/// so this ends.
pub(crate) fn run(sorted: &[(&str, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let ucfg = FactsConfig { unit: true, ..cfg.clone() };
    let mut input = published_surface(sorted, cfg);
    let order = dependency_order(sorted);
    let mut rounds = 0u32;
    let mut round_work: Vec<u64> = Vec::new();
    let mut per_mod: Vec<Option<ProgramFacts>> = (0..sorted.len()).map(|_| None).collect();
    let mut dirty = vec![true; sorted.len()];
    loop {
        rounds += 1;
        input.classes.clear();
        input.fns.clear();
        input.layouts.clear();
        let mut pf = ProgramFacts {
            module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
            dynamic_code_reach: cfg.dynamic_code_reach.clone(),
            sabotage: cfg.sabotage.clone(),
            ..Default::default()
        };
        let mut reports: FxHashSet<String> = FxHashSet::default();
        let mut work = 0u64;
        for &mi in &order {
            let (path, module) = sorted[mi];
            if dirty[mi] {
                let list = [(path.to_string(), module)];
                let u = crate::compute_in(&list, &ucfg, Some(&input));
                work += u.stats.work;
                per_mod[mi] = Some(u);
            }
            let u = per_mod[mi].as_ref().expect("every module solved");
            for (k, c) in &u.contracts_out {
                input.classes.insert(k.clone(), c.clone());
            }
            for (k, c) in &u.fn_contracts_out {
                input.fns.insert(k.clone(), c.clone());
            }
            for c in &u.layouts_out {
                input.layouts.insert(c.display.clone(), c.clone());
            }
            reports.extend(u.reports.iter().cloned());
        }
        round_work.push(work);
        let fresh: Vec<String> = reports.into_iter().filter(|d| !input.broken.contains(d)).collect();
        if !fresh.is_empty() {
            dirty = resolve_again(sorted, &per_mod, &fresh);
            input.broken.extend(fresh);
            continue;
        }
        for (mi, u) in per_mod.into_iter().enumerate() {
            let u = u.expect("every module solved");
            merge_unit(&mut pf, mi, sorted[mi].0, u);
        }
        typed_param_verdicts(&mut pf);
        pf.rounds = rounds;
        // The cost of the run is every round's.
        pf.stats.work = round_work.iter().sum();
        pf.round_work = round_work;
        pf.broken = {
            let mut b: Vec<String> = input.broken.iter().cloned().collect();
            b.sort();
            b
        };
        return pf;
    }
}

/// The units a newly broken contract changes (design §11.3): the unit
/// defining it (a template it owns, or a function it publishes) and every
/// unit importing that one, transitively. A unit over the bound claims and
/// publishes nothing whatever its inputs, so it keeps its result and passes
/// no change on: solving it again would only pay the bound again. A display
/// no unit defines makes every unit dirty.
fn resolve_again(sorted: &[(&str, &Module)], per_mod: &[Option<ProgramFacts>], fresh: &[String]) -> Vec<bool> {
    let n = sorted.len();
    let index: FxHashMap<&str, usize> = sorted.iter().enumerate().map(|(i, (p, _))| (*p, i)).collect();
    let mut owner: FxHashMap<String, usize> = FxHashMap::default();
    for (mi, u) in per_mod.iter().enumerate() {
        let Some(u) = u else { continue };
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            owner.insert(t.display.clone(), mi);
        }
        for (_, c) in &u.fn_contracts_out {
            owner.insert(format!("fn:{}", c.name), mi);
        }
    }
    let mut importers: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (mi, (path, module)) in sorted.iter().enumerate() {
        for imp in &module.imports {
            if let Some(&t) = imp.resolved_path.as_deref().filter(|t| t != path).and_then(|t| index.get(t)) {
                importers[t].push(mi);
            }
        }
    }
    let mut dirty = vec![false; n];
    let mut work: Vec<usize> = Vec::new();
    for d in fresh {
        match owner.get(d) {
            Some(&mi) => work.push(mi),
            None => return vec![true; n],
        }
    }
    while let Some(mi) = work.pop() {
        if per_mod[mi].as_ref().is_some_and(|u| u.stats.aborted) {
            continue;
        }
        if !std::mem::replace(&mut dirty[mi], true) {
            work.extend(importers[mi].iter().copied());
        }
    }
    dirty
}

fn merge_unit(pf: &mut ProgramFacts, mi: usize, path: &str, u: ProgramFacts) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts { module: mi as u32, ..s }));
    pf.bindings.push(u.bindings.into_iter().next().unwrap_or_default());
    pf.super_unrelinked.extend(u.super_unrelinked);
    pf.interp_guard = pf.interp_guard.take().or(u.interp_guard);
    pf.fnproto_bind_written = pf.fnproto_bind_written.take().or(u.fnproto_bind_written);
    let (a, s) = (&mut pf.stats, &u.stats);
    a.modules += s.modules;
    a.templates += s.templates;
    a.fact_units += s.fact_units;
    a.nodes += s.nodes;
    a.constraints += s.constraints;
    a.tokens += s.tokens;
    a.functions += s.functions;
    a.gen_ms += s.gen_ms;
    a.solve_ms += s.solve_ms;
    a.derive_ms += s.derive_ms;
    a.pops += s.pops;
    a.instances += s.instances;
    a.shared_instances += s.shared_instances;
    a.composed += s.composed;
    a.work += s.work;
    a.top_calls += s.top_calls;
    a.pts_entries += s.pts_entries;
    pf.units.push((path.to_string(), u.stats));
}

/// Post-census verdicts for sites on a parameter whose TS type is an
/// object type (design §11.4): the type picks the candidates, every class
/// template of the program whose definite data fields carry the type's
/// keys; the entry guard is `class_id(param) ∈ candidates`. The site is E0
/// when every candidate is prefix-stable and holds the key in one slot.
fn typed_param_verdicts(pf: &mut ProgramFacts) {
    let mut cand_of: FxHashMap<Vec<String>, Vec<usize>> = FxHashMap::default();
    for s in pf.sites.iter_mut() {
        let Some(keys) = s.guard_keys.take() else { continue };
        if !matches!(s.verdict, crate::Verdict::No(_)) || s.is_call {
            continue;
        }
        let tfs = &pf.templates;
        let cands = cand_of.entry(keys.clone()).or_insert_with(|| {
            (0..tfs.len())
                .filter(|&i| {
                    let t = &tfs[i];
                    t.class_id.is_some()
                        && keys.iter().all(|k| t.fields.iter().any(|f| &f.key == k && f.definite && f.data_field))
                })
                .collect()
        });
        if cands.is_empty() {
            continue;
        }
        let slot = |t: &TemplateFacts| {
            t.fields.iter().find(|f| f.key == s.key && f.definite && f.data_field).and_then(|f| f.slot)
        };
        let first = slot(&tfs[cands[0]]);
        if first.is_some() && cands.iter().all(|&i| tfs[i].prefix_stable && slot(&tfs[i]) == first) {
            s.receivers = cands.iter().map(|&i| tfs[i].display.clone()).collect();
            s.verdict = crate::Verdict::E0;
        }
    }
}
