//! The program's export demand: from the graph roots, which exported names
//! some kept item reads, and so which items of each module stay. A module
//! whose exports cannot be followed by name (a dynamic import, a namespace
//! object used as a value, a CommonJS wrap, `eval`) is demanded whole.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use perry_hir::ImportSpecifier;

use super::super::CompilationContext;
use super::summary::{Cond, Eval, Kind, Summary};

/// What the second walk removes from one module.
#[derive(Debug, Clone, Default)]
pub(crate) struct Prune {
    pub fingerprint: u64,
    /// Items (by body index) no kept item and no demanded export needs.
    pub items: BTreeSet<usize>,
    /// Import locals no kept item mentions.
    pub imports: BTreeSet<String>,
    /// Exported names nothing demands (local and re-export specifiers).
    pub exports: BTreeSet<String>,
}

impl Prune {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.imports.is_empty() && self.exports.is_empty()
    }
}

#[derive(Default, Clone)]
struct Demand {
    whole: bool,
    names: BTreeSet<String>,
    /// Names a namespace call reaches (`ns.f()` passes the namespace as `this`).
    via_namespace: BTreeSet<String>,
}

/// Edges of one module: its AST sources resolved, and the HIR edges no item
/// shows (dynamic imports, CommonJS requires).
struct Module<'a> {
    summary: Option<&'a Summary>,
    /// Item index -> resolved target (imports, re-exports, `export *`).
    targets: HashMap<usize, PathBuf>,
    /// Targets every name of which is read: HIR edges no item can follow.
    whole_targets: Vec<PathBuf>,
    /// Plain named HIR imports of a module without a summary.
    named_targets: Vec<(PathBuf, String)>,
    /// Re-export items whose source the collection-time pruner postponed:
    /// left exactly as they are.
    postponed: HashSet<usize>,
}

pub(crate) struct Planned {
    pub prunes: BTreeMap<PathBuf, Prune>,
    pub report: Vec<String>,
}

/// The plan, or `None` when an edge into a compiled module cannot be
/// resolved (then nothing is pruned anywhere).
pub(crate) fn plan(
    ctx: &mut CompilationContext,
    summaries: &HashMap<PathBuf, Summary>,
    roots: &[PathBuf],
) -> Option<Planned> {
    let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
    let key_set: HashSet<PathBuf> = paths.iter().cloned().collect();
    let mut modules: HashMap<PathBuf, Module> = HashMap::new();
    let mut unresolved: Vec<String> = Vec::new();
    for p in &paths {
        let hir_imports = ctx.native_modules[p].imports.clone();
        let hir_exports = ctx.native_modules[p].exports.clone();
        let summary = summaries.get(p).filter(|s| s.prunable);
        let mut m = Module {
            summary,
            targets: HashMap::new(),
            whole_targets: Vec::new(),
            named_targets: Vec::new(),
            postponed: HashSet::new(),
        };
        let resolved = |source: &str| {
            hir_imports
                .iter()
                .find(|i| i.source == source)
                .and_then(|i| i.resolved_path.clone())
        };
        if let Some(s) = summary {
            for (index, item) in s.items.iter().enumerate() {
                let Some(source) = &item.source else { continue };
                if item.kind == Kind::Erased {
                    continue;
                }
                let rp = resolved(source);
                match super::super::reachability::resolve_edge(
                    ctx,
                    p,
                    rp.as_deref(),
                    source,
                    &key_set,
                ) {
                    Some(t) => {
                        m.targets.insert(index, t);
                    }
                    None => match compiled_specifier(ctx, p, source) {
                        // A re-export the collection-time pruner postponed:
                        // nothing demands it and its edge is gone from HIR.
                        Some(t)
                            if matches!(item.kind, Kind::ReExport | Kind::ExportAll)
                                && ctx.reexport_pruner.postponed(&t) =>
                        {
                            m.postponed.insert(index);
                        }
                        Some(t) => unresolved.push(format!(
                            "{} -> {source} -> {}",
                            p.display(),
                            t.display()
                        )),
                        None => {}
                    },
                }
            }
        }
        for import in &hir_imports {
            if import.type_only || import.runtime_erased {
                continue;
            }
            let follows_items = summary.is_some()
                && !import.is_dynamic
                && !import.is_dynamic_target
                && !import.is_deferred_require
                && !import.is_adopted_require;
            if follows_items {
                continue;
            }
            let Some(t) = super::super::reachability::resolve_edge(
                ctx,
                p,
                import.resolved_path.as_deref(),
                &import.source,
                &key_set,
            ) else {
                continue;
            };
            let plain = !import.is_dynamic
                && !import.is_dynamic_target
                && !import.is_deferred_require
                && !import.is_adopted_require;
            let named: Option<Vec<String>> = plain
                .then(|| {
                    import
                        .specifiers
                        .iter()
                        .map(|s| match s {
                            ImportSpecifier::Named { imported, .. } => Some(imported.clone()),
                            ImportSpecifier::Default { .. } => Some("default".to_string()),
                            ImportSpecifier::Namespace { .. } => None,
                        })
                        .collect()
                })
                .flatten();
            match named {
                Some(names) => m
                    .named_targets
                    .extend(names.into_iter().map(|n| (t.clone(), n))),
                None => m.whole_targets.push(t),
            }
        }
        // A module without a summary re-exports what its HIR says.
        if summary.is_none() {
            for export in &hir_exports {
                let source = match export {
                    perry_hir::Export::ReExport { source, .. }
                    | perry_hir::Export::ExportAll { source }
                    | perry_hir::Export::NamespaceReExport { source, .. } => source,
                    perry_hir::Export::Named { .. } => continue,
                };
                let rp = resolved(source);
                if let Some(t) = super::super::reachability::resolve_edge(
                    ctx,
                    p,
                    rp.as_deref(),
                    source,
                    &key_set,
                ) {
                    m.whole_targets.push(t);
                }
            }
        }
        modules.insert(p.clone(), m);
    }

    if !unresolved.is_empty() {
        eprintln!(
            "perry: export demand: {} edges into compiled files outside the graph:",
            unresolved.len()
        );
        for u in unresolved.iter().take(12) {
            eprintln!("perry: export demand:   {u}");
        }
        return None;
    }
    // Import cycles: an imported binding is initialized when its module is
    // not in the reader's cycle (it finished evaluating first).
    let scc = components(&paths, &modules);

    // Roots: the entry and every module no edge enters (workers, extension
    // entries) are demanded whole, as is every module without a summary.
    let mut entered: HashSet<&PathBuf> = HashSet::new();
    for m in modules.values() {
        entered.extend(m.targets.values());
        entered.extend(m.whole_targets.iter());
        entered.extend(m.named_targets.iter().map(|(t, _)| t));
    }
    let mut demand: HashMap<PathBuf, Demand> = HashMap::new();
    let mut work: VecDeque<PathBuf> = VecDeque::new();
    for p in &paths {
        let root = roots.contains(p) || !entered.contains(p) || modules[p].summary.is_none();
        if root {
            demand.entry(p.clone()).or_default().whole = true;
        }
        work.push_back(p.clone());
    }

    let mut live: HashMap<PathBuf, (Vec<bool>, HashSet<String>)> = HashMap::new();
    let mut queued: HashSet<PathBuf> = paths.iter().cloned().collect();
    while let Some(p) = work.pop_front() {
        queued.remove(&p);
        let m = &modules[&p];
        if let Some(s) = m.summary {
            // `ns.f()` passes the namespace as `this`: an `f` that reads
            // `this` can reach any export.
            let d = demand.entry(p.clone()).or_default();
            if !d.whole
                && d.via_namespace.iter().any(|n| {
                    s.items.iter().any(|it| {
                        it.exports
                            .iter()
                            .any(|(e, l)| e == n && s.this_reading.contains(l))
                    })
                })
            {
                d.whole = true;
            }
        }
        let d = demand.get(&p).cloned().unwrap_or_default();
        let mut out: Vec<(PathBuf, Option<String>, bool)> = Vec::new();
        match m.summary {
            None => {
                for t in &m.whole_targets {
                    out.push((t.clone(), None, false));
                }
                for (t, n) in &m.named_targets {
                    out.push((t.clone(), Some(n.clone()), false));
                }
            }
            Some(s) => {
                let (items, names) =
                    live_items(s, &d, |cond| holds(cond, &p, s, m, &modules, &scc));
                // What the kept items read across edges.
                let mut ns_whole: HashSet<&str> = HashSet::new();
                for (i, item) in s.items.iter().enumerate() {
                    if items[i] {
                        for e in &item.escapes {
                            ns_whole.insert(e);
                        }
                    }
                    // An exported namespace import is the namespace object
                    // itself, a value: `import * as ns; export { ns }`.
                    for (exported, local) in &item.exports {
                        if d.whole || d.names.contains(exported) {
                            ns_whole.insert(local);
                        }
                    }
                }
                for (i, item) in s.items.iter().enumerate() {
                    let Some(t) = m.targets.get(&i) else { continue };
                    match item.kind {
                        Kind::Import => {
                            for (local, imported) in &item.imports {
                                if !names.contains(local) {
                                    continue;
                                }
                                match imported {
                                    Some(x) => out.push((t.clone(), Some(x.clone()), false)),
                                    None if ns_whole.contains(local.as_str()) => {
                                        out.push((t.clone(), None, false))
                                    }
                                    None => {
                                        for (j, other) in s.items.iter().enumerate() {
                                            if !items[j] {
                                                continue;
                                            }
                                            for (ns, member) in &other.members {
                                                if ns == local {
                                                    out.push((
                                                        t.clone(),
                                                        Some(member.clone()),
                                                        true,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Kind::ReExport => {
                            for (exported, imported) in &item.reexports {
                                if !(d.whole || d.names.contains(exported)) {
                                    continue;
                                }
                                match imported {
                                    Some(x) => out.push((
                                        t.clone(),
                                        Some(x.clone()),
                                        d.via_namespace.contains(exported),
                                    )),
                                    None => out.push((t.clone(), None, false)),
                                }
                            }
                        }
                        Kind::ExportAll => {
                            if d.whole {
                                out.push((t.clone(), None, false));
                            } else {
                                // A name this module does not declare itself
                                // may come from any `export *` source.
                                let own: HashSet<&String> = s
                                    .items
                                    .iter()
                                    .flat_map(|it| {
                                        it.exports
                                            .iter()
                                            .map(|(e, _)| e)
                                            .chain(it.reexports.iter().map(|(e, _)| e))
                                    })
                                    .collect();
                                for n in &d.names {
                                    if !own.contains(n) {
                                        out.push((
                                            t.clone(),
                                            Some(n.clone()),
                                            d.via_namespace.contains(n),
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                for t in &m.whole_targets {
                    out.push((t.clone(), None, false));
                }
                live.insert(p.clone(), (items, names));
            }
        }
        for (t, name, via_ns) in out {
            let td = demand.entry(t.clone()).or_default();
            let grew = match name {
                None => !std::mem::replace(&mut td.whole, true),
                Some(n) => {
                    let mut g = td.names.insert(n.clone());
                    if via_ns {
                        g |= td.via_namespace.insert(n);
                    }
                    g
                }
            };
            if grew && queued.insert(t.clone()) {
                work.push_back(t);
            }
        }
    }

    let mut prunes = BTreeMap::new();
    let mut report = Vec::new();
    for p in &paths {
        let m = &modules[p];
        let Some(s) = m.summary else { continue };
        let Some((items, names)) = live.get(p) else {
            continue;
        };
        let d = demand.get(p).cloned().unwrap_or_default();
        let mut prune = Prune {
            fingerprint: s.fingerprint,
            ..Default::default()
        };
        for (i, item) in s.items.iter().enumerate() {
            match item.kind {
                Kind::Other if !items[i] => {
                    prune.items.insert(i);
                }
                Kind::Import => {
                    for (local, _) in &item.imports {
                        if !names.contains(local) {
                            prune.imports.insert(local.clone());
                        }
                    }
                }
                Kind::ReExport if m.postponed.contains(&i) => {}
                Kind::ExportLocal | Kind::ReExport if !d.whole => {
                    for e in item
                        .exports
                        .iter()
                        .map(|(e, _)| e)
                        .chain(item.reexports.iter().map(|(e, _)| e))
                    {
                        if !d.names.contains(e) {
                            prune.exports.insert(e.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        report.push(format!(
            "{}\titems {}\tdead {}\timports_dead {}\texports_dead {}\twhole {}",
            p.display(),
            s.items.len(),
            prune.items.len(),
            prune.imports.len(),
            prune.exports.len(),
            d.whole
        ));
        if !prune.is_empty() {
            prunes.insert(p.clone(), prune);
        }
    }
    Some(Planned { prunes, report })
}

/// The kept items of a module under `d`, and the locals they keep.
fn live_items(
    s: &Summary,
    d: &Demand,
    holds: impl Fn(&Cond) -> bool,
) -> (Vec<bool>, HashSet<String>) {
    let n = s.items.len();
    let mut items = vec![false; n];
    let mut names: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = Vec::new();
    let keep = |i: usize, items: &mut Vec<bool>, pending: &mut Vec<String>| {
        if !items[i] {
            items[i] = true;
            pending.extend(s.items[i].mentions.iter().cloned());
        }
    };
    for (i, item) in s.items.iter().enumerate() {
        let inert = match &item.eval {
            Eval::Effect => false,
            Eval::Inert(conds) => conds.iter().all(&holds),
        };
        let structural = matches!(
            item.kind,
            Kind::Import | Kind::ExportLocal | Kind::ReExport | Kind::ExportAll | Kind::Erased
        );
        if structural || !inert {
            keep(i, &mut items, &mut pending);
        }
    }
    // Demanded exports keep their locals.
    for item in &s.items {
        for (exported, local) in &item.exports {
            if d.whole || d.names.contains(exported) {
                pending.push(local.clone());
            }
        }
    }
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, item) in s.items.iter().enumerate() {
        for name in &item.declares {
            by_name.entry(name).or_default().push(i);
        }
    }
    while let Some(name) = pending.pop() {
        if !names.insert(name.clone()) {
            continue;
        }
        if let Some(is) = by_name.get(name.as_str()) {
            for &i in is {
                keep(i, &mut items, &mut pending);
            }
        }
    }
    // An import item's own mentions are its locals: they live only through
    // other items, so an import keeps nothing by itself.
    (items, names)
}

/// A condition of an inert item, checked against the graph.
fn holds(
    cond: &Cond,
    p: &PathBuf,
    s: &Summary,
    m: &Module,
    modules: &HashMap<PathBuf, Module>,
    scc: &HashMap<PathBuf, usize>,
) -> bool {
    let local = match cond {
        Cond::Initialized(l) | Cond::Constructor(l) => l,
    };
    // The import item binding `local`, and its target.
    let Some((index, imported)) = s.items.iter().enumerate().find_map(|(i, item)| {
        item.imports
            .iter()
            .find(|(l, _)| l == local)
            .map(|(_, imported)| (i, imported.clone()))
    }) else {
        return false;
    };
    let Some(target) = m.targets.get(&index) else {
        return false;
    };
    if scc.get(target) == scc.get(p) {
        return false;
    }
    match cond {
        Cond::Initialized(_) => true,
        Cond::Constructor(_) => match imported {
            Some(name) => exports_constructor(target, &name, modules, 0),
            None => false,
        },
    }
}

/// `name` exported by `module` is a class or function declaration.
fn exports_constructor(
    module: &Path,
    name: &str,
    modules: &HashMap<PathBuf, Module>,
    depth: u32,
) -> bool {
    if depth > 32 {
        return false;
    }
    let Some(m) = modules.get(module) else {
        return false;
    };
    let Some(s) = m.summary else { return false };
    for (i, item) in s.items.iter().enumerate() {
        if let Some((_, local)) = item.exports.iter().find(|(e, _)| e == name) {
            return s.constructors.contains(local)
                && !s
                    .items
                    .iter()
                    .any(|it| it.imports.iter().any(|(l, _)| l == local));
        }
        if let Some((_, Some(imported))) = item.reexports.iter().find(|(e, _)| e == name) {
            return m
                .targets
                .get(&i)
                .is_some_and(|t| exports_constructor(t, imported, modules, depth + 1));
        }
    }
    false
}

/// The specifier names a compiled module that the graph does not hold.
fn compiled_specifier(
    ctx: &mut CompilationContext,
    importer: &Path,
    source: &str,
) -> Option<PathBuf> {
    let (p, _) = super::super::resolve::cached_resolve_import(source, importer, ctx)?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    )
    .then(|| p.canonicalize().unwrap_or(p))
}

/// Strongly connected components of the module graph (iterative Tarjan).
fn components(paths: &[PathBuf], modules: &HashMap<PathBuf, Module>) -> HashMap<PathBuf, usize> {
    let index_of: HashMap<&PathBuf, usize> =
        paths.iter().enumerate().map(|(i, p)| (p, i)).collect();
    let succ: Vec<Vec<usize>> = paths
        .iter()
        .map(|p| {
            let m = &modules[p];
            m.targets
                .values()
                .chain(m.whole_targets.iter())
                .chain(m.named_targets.iter().map(|(t, _)| t))
                .filter_map(|t| index_of.get(t).copied())
                .collect()
        })
        .collect();
    let n = paths.len();
    let (mut index, mut low, mut on) = (vec![usize::MAX; n], vec![0usize; n], vec![false; n]);
    let mut comp = vec![usize::MAX; n];
    let (mut next, mut ncomp) = (0usize, 0usize);
    let mut stack: Vec<usize> = Vec::new();
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on[root] = true;
        while let Some(&mut (v, ref mut k)) = call.last_mut() {
            if *k < succ[v].len() {
                let w = succ[v][*k];
                *k += 1;
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on[w] = true;
                    call.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(u, _)) = call.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    while let Some(w) = stack.pop() {
                        on[w] = false;
                        comp[w] = ncomp;
                        if w == v {
                            break;
                        }
                    }
                    ncomp += 1;
                }
            }
        }
    }
    paths.iter().cloned().zip(comp).collect()
}
