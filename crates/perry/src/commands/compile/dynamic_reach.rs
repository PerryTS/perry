//! Dead-code elimination, layer 2 at module level: a module that only
//! dynamic `import()` / `require(expr)` sites reach, each held by a body that
//! cannot run (the whole program's activation pass, DECISIONS 225), is never
//! loaded. It leaves the graph right after collection, with every module only
//! it reaches, so neither facts nor codegen see it. A site whose body cannot
//! run keeps the specifiers of the modules that stay; one left with none
//! lowers to the runtime fallback, as an unresolved specifier does.
//!
//! The roots are the entry, the bundle extension entries and every module no
//! edge enters (workers and the like). Static imports, re-exports and the
//! hoisted requires of CommonJS stay live edges.
//!
//! `PERRY_DYNAMIC_REACH=off` keeps every module (A/B measurement);
//! `PERRY_DYNAMIC_REACH_REPORT=<path>` lists the modules dropped.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::Result;

use super::CompilationContext;

pub(super) fn prune(ctx: &mut CompilationContext, roots: &[PathBuf]) -> Result<()> {
    if matches!(
        std::env::var("PERRY_DYNAMIC_REACH").as_deref(),
        Ok("off" | "0" | "false" | "no")
    ) {
        return Ok(());
    }
    let any_dynamic = ctx.native_modules.values().any(|m| {
        m.imports
            .iter()
            .any(|i| i.is_dynamic || i.is_dynamic_target)
    });
    if !any_dynamic {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let dynamic_code = perry_hir::has_deferred_dynamic_code_sites()
        || ctx.native_module_imports.contains("vm")
        || ctx.uses_data_url_dynamic_import;
    // DECISIONS 235: a real eval-like source site.
    let interpreted_sources = !perry_hir::interpreted_source_sites().is_empty()
        || ctx.native_module_imports.contains("vm")
        || ctx.uses_data_url_dynamic_import;
    let Some(live) = super::program_facts::live_dynamic_imports(
        &ctx.native_modules,
        super::program_facts::export_sources(ctx),
        dynamic_code,
        interpreted_sources,
        &ctx.patched_builtins.dynamic_code_reach,
        roots.iter().map(|p| p.display().to_string()).collect(),
    ) else {
        eprintln!(
            "perry: dynamic reach: the activation pass ran past its bounds, nothing dropped ({:.1}s)",
            started.elapsed().as_secs_f64()
        );
        return Ok(());
    };
    let report = std::env::var_os("PERRY_DYNAMIC_REACH_REPORT");
    let mut lines: Vec<String> = Vec::new();
    let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
    let key_set: HashSet<PathBuf> = paths.iter().cloned().collect();
    // Edges: (target, live).
    let mut edges: HashMap<PathBuf, Vec<(PathBuf, bool)>> = HashMap::new();
    let mut entered: HashSet<PathBuf> = HashSet::new();
    for p in &paths {
        let (imports, exports) = {
            let m = &ctx.native_modules[p];
            (m.imports.clone(), m.exports.clone())
        };
        let name = p.display().to_string();
        let mut out = Vec::new();
        for i in &imports {
            if i.type_only || i.runtime_erased {
                continue;
            }
            let Some(t) = super::reachability::resolve_edge(
                ctx,
                p,
                i.resolved_path.as_deref(),
                &i.source,
                &key_set,
            ) else {
                continue;
            };
            // A synthesized `import()` edge lives when a site a body that
            // can run holds names its specifier.
            let alive = !i.is_dynamic || live.contains(&(name.clone(), i.source.clone()));
            if i.is_dynamic && report.is_some() {
                lines.push(format!(
                    "edge {} {name} -> {}",
                    if alive { "live" } else { "dead" },
                    i.source
                ));
            }
            entered.insert(t.clone());
            out.push((t, alive));
        }
        for e in &exports {
            let source = match e {
                perry_hir::Export::ReExport { source, .. }
                | perry_hir::Export::ExportAll { source }
                | perry_hir::Export::NamespaceReExport { source, .. } => source,
                perry_hir::Export::Named { .. } => continue,
            };
            if let Some(t) = super::reachability::resolve_edge(ctx, p, None, source, &key_set) {
                entered.insert(t.clone());
                out.push((t, true));
            }
        }
        edges.insert(p.clone(), out);
    }
    let mut reached: HashSet<PathBuf> = HashSet::new();
    let mut work: VecDeque<PathBuf> = VecDeque::new();
    for p in &paths {
        if roots.contains(p) || !entered.contains(p) {
            reached.insert(p.clone());
            work.push_back(p.clone());
        }
    }
    while let Some(p) = work.pop_front() {
        for (t, alive) in edges.get(&p).into_iter().flatten() {
            if *alive && reached.insert(t.clone()) {
                work.push_back(t.clone());
            }
        }
    }
    let pruned: HashSet<PathBuf> = paths
        .iter()
        .filter(|p| !reached.contains(*p))
        .cloned()
        .collect();
    let dead_edges = lines.iter().filter(|l| l.starts_with("edge dead")).count();
    eprintln!(
        "perry: dynamic reach: {} dynamic edges, {dead_edges} held only by bodies that cannot run; {} modules dropped ({:.1}s)",
        lines.len(),
        pruned.len(),
        started.elapsed().as_secs_f64()
    );
    if pruned.is_empty() {
        if let Some(path) = &report {
            lines.sort();
            std::fs::write(path, lines.join("\n") + "\n")?;
        }
        return Ok(());
    }
    // Sites (in bodies that cannot run) keep only specifiers of modules that
    // stay.
    let dropped_sources: HashMap<PathBuf, BTreeSet<String>> = paths
        .iter()
        .filter(|p| !pruned.contains(*p))
        .map(|p| {
            let m = &ctx.native_modules[p];
            let gone = m
                .imports
                .iter()
                .filter(|i| {
                    i.is_dynamic
                        && super::reachability::resolve_edge_cached(
                            &key_set,
                            i.resolved_path.as_deref(),
                        )
                        .is_some_and(|t| pruned.contains(&t))
                })
                .map(|i| i.source.clone())
                .collect();
            (p.clone(), gone)
        })
        .collect();
    super::reachability::rewrite_pruned_edges(ctx, &pruned);
    for (p, gone) in &dropped_sources {
        if gone.is_empty() {
            continue;
        }
        if let Some(m) = ctx.native_modules.get_mut(p) {
            perry_hir::for_each_dynamic_import_mut(m, &mut |e| {
                if let perry_hir::Expr::DynamicImport { paths, .. } = e {
                    paths.retain(|s| !gone.contains(s));
                }
            });
        }
    }
    let mut list: Vec<String> = pruned
        .iter()
        .map(|p| format!("dropped {}", p.display()))
        .collect();
    list.sort();
    if let Some(path) = &report {
        lines.sort();
        lines.extend(list);
        std::fs::write(path, lines.join("\n") + "\n")?;
    }
    ctx.native_modules.retain(|p, _| !pruned.contains(p));
    Ok(())
}
