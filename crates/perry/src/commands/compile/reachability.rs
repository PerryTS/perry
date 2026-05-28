//! #2309 Stage 1: tree-shaking / dead-code elimination over the module graph.
//!
//! Perry collects every import-reachable module at module granularity and
//! refuses (during lowering) on genuinely-runtime `new Function` and
//! unimplemented APIs — even when the offending module is only reachable via a
//! dead re-export-barrel edge and never actually runs. This pass, run after
//! the full graph is collected (and after `rerun_collect_with_class_field_types`
//! so it operates on the final `native_modules`), computes binding-level
//! reachability from the user code and prunes unreachable `node_modules`
//! modules before codegen. Refusals deferred during collection
//! (see [`perry_hir::deferral`]) are then re-raised only for modules that
//! survive — so dead code's refusals are dropped, live code's are still fatal.
//!
//! ## Model
//!
//! Each module reaches one of two states in a monotone fixpoint:
//! - **Rooted** — the whole module is live (its init runs and every static
//!   import edge is followed). All user (non-`node_modules`) modules are
//!   seeded Rooted, so user code is never pruned and we only ever drop dead
//!   transitive dependencies.
//! - **Bindings(set)** — only specific exported names are needed. We keep this
//!   sub-module precision **only** for *pure re-export barrels* in packages
//!   marked `"sideEffects": false` (the spec-sanctioned signal). For these we
//!   follow only the import edges feeding the needed exports and drop bare
//!   side-effect imports — exactly what removes `es-toolkit`'s `template.mjs`
//!   (`new Function`) when a consumer imports only `throttle`. Any binding
//!   demand on any other module escalates it to Rooted (module granularity).
//!
//! All ambiguous shapes (namespace import, `export *`, dynamic `import()`,
//! unresolvable specifiers) conservatively mark the whole target Rooted. The
//! default is always "keep"; pruning only happens on precise binding paths.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Result;
use perry_hir::{Export, Import, ImportSpecifier};

use super::resolve::cached_resolve_import;
use super::{CompilationContext, SideEffects};

/// Entry point: prune unreachable `node_modules` modules and re-raise any
/// deferred refusal that survives. No-op (byte-identical to pre-#2309) unless
/// `ctx.tree_shake` is set.
pub(super) fn tree_shake(ctx: &mut CompilationContext, entry_canonical: &Path) -> Result<()> {
    if !ctx.tree_shake {
        return Ok(());
    }

    let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
    let key_set: HashSet<PathBuf> = paths.iter().cloned().collect();

    // --- snapshot per-module metadata (imports/exports + barrel-ness) ---
    let mut barrel: HashMap<PathBuf, bool> = HashMap::new();
    let mut raw: HashMap<PathBuf, (Vec<Import>, Vec<Export>)> = HashMap::new();
    for p in &paths {
        let m = &ctx.native_modules[p];
        barrel.insert(p.clone(), is_pure_reexport_barrel(m));
        raw.insert(p.clone(), (m.imports.clone(), m.exports.clone()));
    }

    // --- resolve every edge target to a native_modules key + precompute the
    //     `sideEffects: false` flag per module (needs &mut ctx for caches;
    //     native_modules is not borrowed here since we cloned above) ---
    let mut se_none: HashMap<PathBuf, bool> = HashMap::new();
    for p in &paths {
        se_none.insert(
            p.clone(),
            matches!(side_effects_of(ctx, p), SideEffects::None),
        );
    }

    let mut graph: HashMap<PathBuf, ResolvedModule> = HashMap::new();
    for p in &paths {
        let (imports, exports) = &raw[p];
        let mut rimports = Vec::new();
        for imp in imports {
            if imp.type_only {
                continue;
            }
            let target = resolve_edge(ctx, p, imp.resolved_path.as_deref(), &imp.source, &key_set);
            let target_se_none = target.as_ref().map(|t| se_none[t]).unwrap_or(false);
            // One reachability edge per specifier so every imported name is
            // demanded (collapsing a multi-specifier decl to one edge would
            // under-demand and risk pruning a live module).
            for kind in specifier_kinds(&imp.specifiers) {
                rimports.push(ResolvedImport {
                    target: target.clone(),
                    target_se_none,
                    kind,
                    is_dynamic: imp.is_dynamic,
                });
            }
        }
        let mut rexports = Vec::new();
        for exp in exports {
            rexports.push(resolve_export(ctx, p, exp, &key_set));
        }
        graph.insert(
            p.clone(),
            ResolvedModule {
                imports: rimports,
                exports: rexports,
            },
        );
    }

    // --- fixpoint ---
    // Seed: every user (non-node_modules) module is a root, so user code is
    // never pruned and bundle-extension / multi-entry roots are covered.
    let mut roots: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !is_in_node_modules(p))
        .cloned()
        .collect();
    roots.push(entry_canonical.to_path_buf());
    let reachable = compute_reachable(&graph, &barrel, &se_none, &roots);

    // --- prune unreachable node_modules modules (never user code) ---
    let pruned: HashSet<PathBuf> = paths
        .iter()
        .filter(|p| !reachable.contains(*p) && is_in_node_modules(p))
        .cloned()
        .collect();

    if pruned.is_empty() {
        return reraise_surviving_refusals(ctx, &key_set);
    }

    // Rewrite surviving modules so no import/export edge points at a pruned
    // module (only pure barrels reached via Bindings can have such dangling
    // edges; init-order already tolerates missing targets but codegen of a
    // re-export to a pruned module would dangle). Drop those edges + the
    // exports that depended on them — safe, since a pruned target was never
    // demanded by live code.
    rewrite_pruned_edges(ctx, &pruned);

    let before = ctx.native_modules.len();
    ctx.native_modules.retain(|p, _| !pruned.contains(p));
    let removed = before - ctx.native_modules.len();
    if removed > 0 {
        if let Ok(v) = std::env::var("PERRY_TREE_SHAKE_DIAG") {
            if v != "0" {
                eprintln!("[tree-shake] pruned {removed} unreachable node_modules module(s)");
            }
        }
    }

    let surviving: HashSet<PathBuf> = ctx.native_modules.keys().cloned().collect();
    reraise_surviving_refusals(ctx, &surviving)
}

/// The pure reachability fixpoint, decoupled from `CompilationContext` for
/// testability. Returns the set of modules that must be compiled.
fn compute_reachable(
    graph: &HashMap<PathBuf, ResolvedModule>,
    barrel: &HashMap<PathBuf, bool>,
    se_none: &HashMap<PathBuf, bool>,
    roots: &[PathBuf],
) -> HashSet<PathBuf> {
    let mut state: HashMap<PathBuf, St> = HashMap::new();
    let mut work: VecDeque<PathBuf> = VecDeque::new();
    for r in roots {
        demand_root(&mut state, &mut work, r);
    }
    let mut reachable: HashSet<PathBuf> = HashSet::new();
    while let Some(p) = work.pop_front() {
        let Some(rm) = graph.get(&p).cloned() else {
            continue;
        };
        match state.get(&p).cloned() {
            Some(St::Rooted) => {
                reachable.insert(p.clone());
                follow_all(&mut state, &mut work, &rm);
            }
            Some(St::Bindings(needed)) => {
                reachable.insert(p.clone());
                let is_barrel =
                    *barrel.get(&p).unwrap_or(&false) && *se_none.get(&p).unwrap_or(&false);
                if is_barrel {
                    for name in &needed {
                        follow_export_binding(&mut state, &mut work, &p, &rm, name);
                    }
                } else {
                    // Module granularity: any binding demand on a non-barrel
                    // module makes the whole module live.
                    demand_root(&mut state, &mut work, &p);
                }
            }
            None => {}
        }
    }
    reachable
}

/// Re-raise the first deferred refusal whose module survived the prune. Dead
/// modules' refusals are dropped silently — the offending code never ships.
fn reraise_surviving_refusals(
    ctx: &CompilationContext,
    surviving: &HashSet<PathBuf>,
) -> Result<()> {
    for d in &ctx.deferred_refusals {
        let module_path = PathBuf::from(&d.module);
        if surviving.contains(&module_path) {
            let loc = d
                .line
                .map(|l| format!("{}:{}", d.module, l))
                .unwrap_or_else(|| d.module.clone());
            return Err(anyhow::anyhow!("{}\n  in {}", d.message, loc));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolved graph types
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ResolvedModule {
    imports: Vec<ResolvedImport>,
    exports: Vec<ResolvedExport>,
}

#[derive(Clone)]
struct ResolvedImport {
    /// Resolved native_modules key, if the target is a compiled module.
    target: Option<PathBuf>,
    /// Target package declares `sideEffects: false`.
    target_se_none: bool,
    kind: ImpKind,
    is_dynamic: bool,
}

#[derive(Clone)]
enum ImpKind {
    /// `import { imported as local }` — carries the imported (source) name.
    Named(String),
    Default,
    Namespace,
    /// Bare side-effect import `import "x"` (no specifiers).
    Bare,
}

#[derive(Clone)]
struct ResolvedExport {
    target: Option<PathBuf>,
    kind: ExpKind,
}

#[derive(Clone)]
enum ExpKind {
    /// `export { imported as exported } from "src"`.
    ReExport { imported: String, exported: String },
    /// `export { local as exported }` (no source).
    Named { local: String, exported: String },
    /// `export * from "src"`.
    Star,
    /// `export * as ns from "src"`.
    NamespaceStar,
}

/// One [`ImpKind`] edge per specifier (Bare when the decl has none).
fn specifier_kinds(specs: &[ImportSpecifier]) -> Vec<ImpKind> {
    if specs.is_empty() {
        return vec![ImpKind::Bare];
    }
    specs
        .iter()
        .map(|s| match s {
            ImportSpecifier::Named { imported, .. } => ImpKind::Named(imported.clone()),
            ImportSpecifier::Default { .. } => ImpKind::Default,
            ImportSpecifier::Namespace { .. } => ImpKind::Namespace,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fixpoint demand helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum St {
    Bindings(HashSet<String>),
    Rooted,
}

fn demand_root(state: &mut HashMap<PathBuf, St>, work: &mut VecDeque<PathBuf>, p: &Path) {
    match state.get(p) {
        Some(St::Rooted) => {}
        _ => {
            state.insert(p.to_path_buf(), St::Rooted);
            work.push_back(p.to_path_buf());
        }
    }
}

fn demand_binding(
    state: &mut HashMap<PathBuf, St>,
    work: &mut VecDeque<PathBuf>,
    p: &Path,
    name: &str,
) {
    match state.get_mut(p) {
        Some(St::Rooted) => {}
        Some(St::Bindings(s)) => {
            if s.insert(name.to_string()) {
                work.push_back(p.to_path_buf());
            }
        }
        None => {
            let mut s = HashSet::new();
            s.insert(name.to_string());
            state.insert(p.to_path_buf(), St::Bindings(s));
            work.push_back(p.to_path_buf());
        }
    }
}

/// Follow every edge of a Rooted module.
fn follow_all(state: &mut HashMap<PathBuf, St>, work: &mut VecDeque<PathBuf>, rm: &ResolvedModule) {
    for imp in &rm.imports {
        let Some(target) = &imp.target else { continue };
        if imp.is_dynamic {
            demand_root(state, work, target);
            continue;
        }
        match &imp.kind {
            ImpKind::Named(imported) => demand_binding(state, work, target, imported),
            ImpKind::Default => demand_binding(state, work, target, "default"),
            ImpKind::Namespace => demand_root(state, work, target),
            ImpKind::Bare => {
                // A bare side-effect import is droppable only if the target
                // package declares `sideEffects: false`; otherwise its
                // top-level side effect must run, so keep it.
                if !imp.target_se_none {
                    demand_root(state, work, target);
                }
            }
        }
    }
    for exp in &rm.exports {
        let Some(target) = &exp.target else { continue };
        match &exp.kind {
            ExpKind::ReExport { imported, .. } => demand_binding(state, work, target, imported),
            ExpKind::Star | ExpKind::NamespaceStar => demand_root(state, work, target),
            ExpKind::Named { .. } => {}
        }
    }
}

/// Follow only the edges feeding a single needed export of a pure barrel.
fn follow_export_binding(
    state: &mut HashMap<PathBuf, St>,
    work: &mut VecDeque<PathBuf>,
    p: &Path,
    rm: &ResolvedModule,
    name: &str,
) {
    // 1. explicit re-export of `name`?
    for exp in &rm.exports {
        match &exp.kind {
            ExpKind::ReExport { exported, imported } if exported == name => {
                if let Some(target) = &exp.target {
                    demand_binding(state, work, target, imported);
                }
                return;
            }
            ExpKind::Named { local, exported } if exported == name => {
                // import-then-export: find the import binding `local`.
                if follow_import_local(state, work, rm, local) {
                    return;
                }
                // local def in a "pure barrel" shouldn't happen; be safe.
                demand_root(state, work, p);
                return;
            }
            _ => {}
        }
    }
    // 2. not an explicit export — could come from a star export.
    let mut had_star = false;
    for exp in &rm.exports {
        if matches!(exp.kind, ExpKind::Star | ExpKind::NamespaceStar) {
            if let Some(target) = &exp.target {
                had_star = true;
                demand_root(state, work, target);
            }
        }
    }
    if !had_star {
        // Can't resolve the name precisely — keep the whole module.
        demand_root(state, work, p);
    }
}

/// Follow the import edge that binds `local` in a barrel. Returns true if an
/// import provided it.
fn follow_import_local(
    state: &mut HashMap<PathBuf, St>,
    work: &mut VecDeque<PathBuf>,
    rm: &ResolvedModule,
    local: &str,
) -> bool {
    for imp in &rm.imports {
        let Some(target) = &imp.target else { continue };
        match &imp.kind {
            // import_kind collapses a decl's named specifiers to the first
            // imported name; for the barrel re-export case the local equals the
            // imported name (`export { x }` from `import { x }`), so demand the
            // same name on the target. When they differ we conservatively
            // demand `local` too.
            ImpKind::Named(imported) if imported == local => {
                demand_binding(state, work, target, imported);
                return true;
            }
            _ => {}
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Edge rewriting (drop references to pruned modules from survivors)
// ---------------------------------------------------------------------------

fn rewrite_pruned_edges(ctx: &mut CompilationContext, pruned: &HashSet<PathBuf>) {
    let surviving: Vec<PathBuf> = ctx
        .native_modules
        .keys()
        .filter(|p| !pruned.contains(*p))
        .cloned()
        .collect();
    // native_modules keys after prune-decision (pruned not yet removed).
    let key_set: HashSet<PathBuf> = ctx.native_modules.keys().cloned().collect();
    for p in surviving {
        let (imports, exports) = {
            let m = &ctx.native_modules[&p];
            (m.imports.clone(), m.exports.clone())
        };

        // Which imports point at a pruned module? Drop them and record the
        // local names they bound (so dependent exports go too).
        let mut import_keep = Vec::with_capacity(imports.len());
        let mut dropped_locals: HashSet<String> = HashSet::new();
        for imp in &imports {
            let target = resolve_edge(ctx, &p, imp.resolved_path.as_deref(), &imp.source, &key_set);
            let drop = target.as_ref().map(|t| pruned.contains(t)).unwrap_or(false);
            import_keep.push(!drop);
            if drop {
                for s in &imp.specifiers {
                    match s {
                        ImportSpecifier::Named { local, .. }
                        | ImportSpecifier::Default { local }
                        | ImportSpecifier::Namespace { local } => {
                            dropped_locals.insert(local.clone());
                        }
                    }
                }
            }
        }

        // Which exports reference a pruned source, or a now-dropped local?
        let mut export_keep = Vec::with_capacity(exports.len());
        for exp in &exports {
            let keep = match exp {
                Export::ReExport { source, .. }
                | Export::ExportAll { source }
                | Export::NamespaceReExport { source, .. } => {
                    let t = resolve_edge(ctx, &p, None, source, &key_set);
                    !t.as_ref().map(|t| pruned.contains(t)).unwrap_or(false)
                }
                Export::Named { local, .. } => !dropped_locals.contains(local),
            };
            export_keep.push(keep);
        }

        if import_keep.iter().all(|k| *k) && export_keep.iter().all(|k| *k) {
            continue; // nothing to rewrite
        }

        let m = ctx.native_modules.get_mut(&p).unwrap();
        let mut ii = 0;
        m.imports.retain(|_| {
            let keep = import_keep[ii];
            ii += 1;
            keep
        });
        let mut ei = 0;
        m.exports.retain(|_| {
            let keep = export_keep[ei];
            ei += 1;
            keep
        });
    }
}

// ---------------------------------------------------------------------------
// Resolution + classification helpers
// ---------------------------------------------------------------------------

/// Resolve an import/export edge to its canonical `native_modules` key, if the
/// target is a compiled module present in the graph. Prefers the precomputed
/// `resolved_path` (set during collection), falling back to the resolver.
fn resolve_edge(
    ctx: &mut CompilationContext,
    importer: &Path,
    resolved_path: Option<&str>,
    source: &str,
    key_set: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    if let Some(rp) = resolved_path {
        let canon = PathBuf::from(rp);
        let canon = canon.canonicalize().unwrap_or(canon);
        if key_set.contains(&canon) {
            return Some(canon);
        }
    }
    let (p, _kind) = cached_resolve_import(source, importer, ctx)?;
    let canon = p.canonicalize().unwrap_or(p);
    if key_set.contains(&canon) {
        Some(canon)
    } else {
        None
    }
}

fn resolve_export(
    ctx: &mut CompilationContext,
    importer: &Path,
    exp: &Export,
    key_set: &HashSet<PathBuf>,
) -> ResolvedExport {
    match exp {
        Export::ReExport {
            source,
            imported,
            exported,
        } => ResolvedExport {
            target: resolve_edge(ctx, importer, None, source, key_set),
            kind: ExpKind::ReExport {
                imported: imported.clone(),
                exported: exported.clone(),
            },
        },
        Export::Named { local, exported } => ResolvedExport {
            target: None,
            kind: ExpKind::Named {
                local: local.clone(),
                exported: exported.clone(),
            },
        },
        Export::ExportAll { source } => ResolvedExport {
            target: resolve_edge(ctx, importer, None, source, key_set),
            kind: ExpKind::Star,
        },
        Export::NamespaceReExport { source, .. } => ResolvedExport {
            target: resolve_edge(ctx, importer, None, source, key_set),
            kind: ExpKind::NamespaceStar,
        },
    }
}

/// A pure re-export barrel: no executable definitions or top-level statements,
/// only imports + exports. Edge-skipping is sound only for these (their entire
/// observable behaviour *is* their re-export list).
fn is_pure_reexport_barrel(m: &perry_hir::Module) -> bool {
    m.functions.is_empty()
        && m.classes.is_empty()
        && m.globals.is_empty()
        && m.enums.is_empty()
        && m.init.is_empty()
        && m.exported_native_instances.is_empty()
        && m.exported_func_return_native_instances.is_empty()
        && m.widgets.is_empty()
        && m.extern_funcs.is_empty()
}

fn is_in_node_modules(p: &Path) -> bool {
    p.to_string_lossy().contains("node_modules")
}

/// Read (and cache) a module's package `sideEffects` field, walking up to the
/// nearest `package.json`. Absent / `true` / array-of-globs ⇒ `Unknown`
/// (conservative; never drops). Only `false` ⇒ `None`.
fn side_effects_of(ctx: &mut CompilationContext, module_path: &Path) -> SideEffects {
    // Find nearest package.json dir.
    let mut dir = module_path.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        let candidate = d.join("package.json");
        if candidate.exists() {
            if let Some(cached) = ctx.side_effects_cache.get(&d) {
                return cached.clone();
            }
            let se = parse_side_effects(&candidate);
            ctx.side_effects_cache.insert(d.clone(), se.clone());
            return se;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    SideEffects::Unknown
}

fn parse_side_effects(pkg_json: &Path) -> SideEffects {
    let Ok(content) = std::fs::read_to_string(pkg_json) else {
        return SideEffects::Unknown;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return SideEffects::Unknown;
    };
    match val.get("sideEffects") {
        Some(serde_json::Value::Bool(false)) => SideEffects::None,
        Some(serde_json::Value::Array(arr)) => {
            // Conservative for PR1: a glob list means *some* files have side
            // effects; without per-file glob matching we keep the package
            // side-effectful (never drop). Captured as Globs for future use.
            let globs = arr
                .iter()
                .filter_map(|g| g.as_str().map(str::to_string))
                .collect();
            SideEffects::Globs(globs)
        }
        _ => SideEffects::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn named(imported: &str, target: &str, se_none: bool) -> ResolvedImport {
        ResolvedImport {
            target: Some(p(target)),
            target_se_none: se_none,
            kind: ImpKind::Named(imported.to_string()),
            is_dynamic: false,
        }
    }
    fn namespace(target: &str) -> ResolvedImport {
        ResolvedImport {
            target: Some(p(target)),
            target_se_none: false,
            kind: ImpKind::Namespace,
            is_dynamic: false,
        }
    }
    fn bare(target: &str, se_none: bool) -> ResolvedImport {
        ResolvedImport {
            target: Some(p(target)),
            target_se_none: se_none,
            kind: ImpKind::Bare,
            is_dynamic: false,
        }
    }
    fn exp_named(local: &str, exported: &str) -> ResolvedExport {
        ResolvedExport {
            target: None,
            kind: ExpKind::Named {
                local: local.to_string(),
                exported: exported.to_string(),
            },
        }
    }
    fn exp_star(target: &str) -> ResolvedExport {
        ResolvedExport {
            target: Some(p(target)),
            kind: ExpKind::Star,
        }
    }
    fn module(imports: Vec<ResolvedImport>, exports: Vec<ResolvedExport>) -> ResolvedModule {
        ResolvedModule { imports, exports }
    }

    /// A pure `sideEffects:false` barrel: importing one named binding drops
    /// the unused leaf (and its `new Function`) plus the bare side-effect edge.
    #[test]
    fn barrel_drops_unused_leaf_and_bare_import() {
        let mut graph = HashMap::new();
        graph.insert(
            p("entry"),
            module(vec![named("throttle", "barrel", true)], vec![]),
        );
        graph.insert(
            p("barrel"),
            module(
                vec![
                    named("throttle", "throttle.mjs", true),
                    named("template", "template.mjs", true),
                    bare("compat.mjs", true),
                ],
                vec![
                    exp_named("throttle", "throttle"),
                    exp_named("template", "template"),
                ],
            ),
        );
        graph.insert(p("throttle.mjs"), module(vec![], vec![]));
        graph.insert(p("template.mjs"), module(vec![], vec![]));
        graph.insert(
            p("compat.mjs"),
            module(vec![named("template", "template.mjs", true)], vec![]),
        );

        let mut barrel = HashMap::new();
        barrel.insert(p("barrel"), true);
        let mut se = HashMap::new();
        for m in ["barrel", "throttle.mjs", "template.mjs", "compat.mjs"] {
            se.insert(p(m), true);
        }

        let r = compute_reachable(&graph, &barrel, &se, &[p("entry")]);
        assert!(r.contains(&p("entry")));
        assert!(r.contains(&p("barrel")));
        assert!(r.contains(&p("throttle.mjs")));
        assert!(
            !r.contains(&p("template.mjs")),
            "unused leaf must be pruned"
        );
        assert!(
            !r.contains(&p("compat.mjs")),
            "bare sideEffects:false import must be dropped"
        );
    }

    /// A namespace import of the same barrel keeps everything (we can't tell
    /// which members are used) — except a bare `sideEffects:false` edge.
    #[test]
    fn namespace_import_keeps_all_named_leaves() {
        let mut graph = HashMap::new();
        graph.insert(p("entry"), module(vec![namespace("barrel")], vec![]));
        graph.insert(
            p("barrel"),
            module(
                vec![
                    named("throttle", "throttle.mjs", true),
                    named("template", "template.mjs", true),
                    bare("compat.mjs", true),
                ],
                vec![
                    exp_named("throttle", "throttle"),
                    exp_named("template", "template"),
                ],
            ),
        );
        graph.insert(p("throttle.mjs"), module(vec![], vec![]));
        graph.insert(p("template.mjs"), module(vec![], vec![]));
        graph.insert(p("compat.mjs"), module(vec![], vec![]));
        let mut barrel = HashMap::new();
        barrel.insert(p("barrel"), true);
        let mut se = HashMap::new();
        for m in ["barrel", "throttle.mjs", "template.mjs", "compat.mjs"] {
            se.insert(p(m), true);
        }
        let r = compute_reachable(&graph, &barrel, &se, &[p("entry")]);
        assert!(
            r.contains(&p("template.mjs")),
            "namespace import keeps named leaves"
        );
        assert!(r.contains(&p("throttle.mjs")));
        assert!(
            !r.contains(&p("compat.mjs")),
            "bare sideEffects:false edge still droppable"
        );
    }

    /// `export *` from a barrel forces the star source live (can't resolve the
    /// member precisely) — no false prune.
    #[test]
    fn star_export_keeps_source() {
        let mut graph = HashMap::new();
        graph.insert(p("entry"), module(vec![named("x", "barrel", true)], vec![]));
        graph.insert(p("barrel"), module(vec![], vec![exp_star("leaf")]));
        graph.insert(p("leaf"), module(vec![], vec![]));
        let mut barrel = HashMap::new();
        barrel.insert(p("barrel"), true);
        let mut se = HashMap::new();
        se.insert(p("barrel"), true);
        se.insert(p("leaf"), true);
        let r = compute_reachable(&graph, &barrel, &se, &[p("entry")]);
        assert!(r.contains(&p("leaf")), "export* source must stay live");
    }

    /// A binding demand on a NON-barrel module escalates it to whole-module
    /// (module granularity), following all its edges.
    #[test]
    fn non_barrel_binding_escalates_to_rooted() {
        let mut graph = HashMap::new();
        graph.insert(p("entry"), module(vec![named("a", "lib", false)], vec![]));
        graph.insert(
            p("lib"),
            module(
                vec![named("dep", "dep.mjs", false)],
                vec![exp_named("a", "a")],
            ),
        );
        graph.insert(p("dep.mjs"), module(vec![], vec![]));
        let barrel = HashMap::new(); // lib is NOT a barrel
        let se = HashMap::new();
        let r = compute_reachable(&graph, &barrel, &se, &[p("entry")]);
        assert!(r.contains(&p("lib")));
        assert!(
            r.contains(&p("dep.mjs")),
            "non-barrel binding pulls whole module"
        );
    }
}
