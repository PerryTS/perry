//! Module-init reachability + topo-sort helpers extracted from
//! `run_with_parse_cache`.
//!
//! Two passes that decide the order in which Perry's compiled module
//! initializers run at program start:
//!
//! - `classify_eager_modules` (issue #753): fixed-point reachability from
//!   the entry. Modules reached through static-import or re-export edges
//!   init at program start (Eager); modules reached only through dynamic
//!   `import()` edges init lazily on first dispatch (Deferred).
//! - `topo_sort_non_entry_modules`: DFS topological sort by import
//!   dependencies, so a module that imports from another module runs
//!   after that other module's initializer. Cycles are broken at the
//!   back-edge.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::OutputFormat;

use super::resolve::resolve_import;
use super::CompilationContext;

/// Issue #753: reachability classification for eager vs deferred init.
/// Modules reachable from the entry through any static-import or
/// re-export edge init at program start (Eager). Modules reachable
/// ONLY through dynamic `import()` edges init lazily on first
/// dispatch (Deferred). Run a fixed-point pass starting from the
/// entry and propagating Eager across static / re-export edges; what
/// remains unmarked is Deferred. Re-export sources must propagate
/// because an Eager module's namespace populator reads the source's
/// getter at init time — if the source is Deferred, the getter
/// returns a zero-initialized global rather than the real binding.
pub(super) fn classify_eager_modules(ctx: &mut CompilationContext, entry_path: &Path) {
    let mut eager: HashSet<PathBuf> = HashSet::new();
    eager.insert(entry_path.to_path_buf());
    // Next.js wall 54 (part 2): the `.next/server/**` page/route/chunk modules
    // have no static importer (loaded by a runtime-computed path). They are left
    // Deferred — eager-initing turbopack chunks at startup runs React-SSR code
    // before the server is ready. Instead, the entry registers each module's
    // `__init` address by path (`js_register_path_init`), so the first runtime
    // `require(absolutePath)` (turbopack `R.c()` / `require(getPagePath(...))`)
    // triggers init lazily and in dependency order.
    loop {
        let mut changed = false;
        let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
        for path in &paths {
            if !eager.contains(path) {
                continue;
            }
            let module = match ctx.native_modules.get(path) {
                Some(m) => m,
                None => continue,
            };
            let static_targets: Vec<PathBuf> = module
                .imports
                .iter()
                .filter(|i| !i.is_dynamic && !i.type_only && !i.is_deferred_require)
                .filter_map(|i| i.resolved_path.as_ref().map(PathBuf::from))
                .collect();
            let reexport_sources: Vec<String> = module
                .exports
                .iter()
                .filter_map(|e| match e {
                    perry_hir::Export::ExportAll { source } => Some(source.clone()),
                    perry_hir::Export::ReExport { source, .. } => Some(source.clone()),
                    perry_hir::Export::NamespaceReExport { source, .. } => Some(source.clone()),
                    perry_hir::Export::Named { .. } => None,
                })
                .collect();
            for resolved_path in static_targets {
                if ctx.native_modules.contains_key(&resolved_path)
                    && !eager.contains(&resolved_path)
                {
                    eager.insert(resolved_path);
                    changed = true;
                }
            }
            for src in reexport_sources {
                if let Some((resolved_path, _)) = resolve_import(
                    &src,
                    path,
                    &ctx.project_root,
                    &ctx.compile_packages,
                    &ctx.compile_package_dirs,
                ) {
                    if ctx.native_modules.contains_key(&resolved_path)
                        && !eager.contains(&resolved_path)
                    {
                        eager.insert(resolved_path);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (path, module) in ctx.native_modules.iter_mut() {
        module.init_kind = if eager.contains(path) {
            perry_hir::ModuleInitKind::Eager
        } else {
            perry_hir::ModuleInitKind::Deferred
        };
    }
}

/// Collect non-entry module names for init function calls.
///
/// Topologically sort by import dependencies so that if module A imports from module B,
/// module B is initialized first. This ensures module-level variables (e.g., Maps) are
/// allocated before other modules try to use them via imported functions.
pub(super) fn topo_sort_non_entry_modules(
    ctx: &CompilationContext,
    entry_path: &Path,
    format: OutputFormat,
    verbose: u8,
) -> Vec<String> {
    // Build path->name mapping and dependency graph
    let mut path_to_name: HashMap<PathBuf, String> = HashMap::new();
    let mut name_to_path: HashMap<String, PathBuf> = HashMap::new();
    let mut deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    for (path, hir_module) in &ctx.native_modules {
        if *path == *entry_path {
            continue;
        }
        path_to_name.insert(path.clone(), hir_module.name.clone());
        name_to_path.insert(hir_module.name.clone(), path.clone());

        let mut module_deps = Vec::new();
        for import in &hir_module.imports {
            // Issue #680: skip whole-decl type-only imports
            // (`import type * as X`, `import type { Foo } from "..."`).
            // Type-only imports are erased at runtime — they MUST NOT
            // be init-order edges. Pre-fix Effect's
            // `internal/tracer.ts` had a `import type * as Tracer`
            // self-edge that combined with Tracer.ts's value
            // `import * as internal from "./internal/tracer.js"` to
            // form a phantom cycle. The DFS cycle-break direction
            // then put `internal/tracer.ts` ahead of `Context.ts`
            // (transitively reached via the same phony edge chain),
            // so tracer's top-level `Context.Reference()(...)` ran
            // against an uninitialized Context global and threw.
            // `is_deferred_require`: a function-local `require('S')` is not an
            // init-order edge — S inits lazily when the require shim runs, not
            // as part of this module's eager init.
            if import.type_only || import.is_deferred_require {
                continue;
            }
            if let Some(ref resolved) = import.resolved_path {
                let resolved_path = PathBuf::from(resolved);
                if resolved_path != *entry_path && ctx.native_modules.contains_key(&resolved_path) {
                    module_deps.push(resolved_path);
                }
            }
        }
        // Also treat ExportAll/ReExport sources as dependencies.
        // If module A does `export * from './B'`, then B must be initialized before A
        // so that B's export globals are set before any consumer of A reads them.
        for export in &hir_module.exports {
            let source = match export {
                perry_hir::Export::ExportAll { source } => Some(source),
                perry_hir::Export::ReExport { source, .. } => Some(source),
                // #310 — namespace re-export's target file must also be
                // initialized before this re-exporter so consumers see
                // populated export globals when they reach through.
                perry_hir::Export::NamespaceReExport { source, .. } => Some(source),
                perry_hir::Export::Named { .. } => None,
            };
            if let Some(src) = source {
                if let Some((resolved_path, _)) = resolve_import(
                    src,
                    path,
                    &ctx.project_root,
                    &ctx.compile_packages,
                    &ctx.compile_package_dirs,
                ) {
                    if resolved_path != *entry_path
                        && ctx.native_modules.contains_key(&resolved_path)
                    {
                        module_deps.push(resolved_path);
                    }
                }
            }
        }
        deps.insert(path.clone(), module_deps);
    }

    // DFS-based topological sort (handles circular dependencies gracefully)
    // Dependencies are visited before the module itself. Cycles are broken
    // at the back-edge (module already being visited), ensuring the best
    // possible ordering even with circular imports.
    let mut sorted = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut visiting: HashSet<PathBuf> = HashSet::new(); // cycle detection

    fn dfs_visit(
        path: &PathBuf,
        deps: &HashMap<PathBuf, Vec<PathBuf>>,
        path_to_name: &HashMap<PathBuf, String>,
        visited: &mut HashSet<PathBuf>,
        visiting: &mut HashSet<PathBuf>,
        sorted: &mut Vec<String>,
    ) {
        if visited.contains(path) || visiting.contains(path) {
            return; // already done or cycle back-edge
        }
        visiting.insert(path.clone());

        // Visit dependencies first (so they get initialized before us).
        //
        // #6463 follow-up (effect web.ts "Not a valid effect: undefined"):
        // visit deps in IMPORT-DECLARATION order, not alphabetically. For a
        // DAG the two produce equally valid topological orders, but inside a
        // cycle the visit order decides WHICH edge becomes the broken
        // back-edge — i.e. which module's body runs first. Node's ESM
        // evaluation visits requested modules in declaration order, so an
        // alphabetical order here can break a cycle in the OPPOSITE
        // direction from node: the module node evaluates first is ordered
        // last by perry, its init-call edge is dropped as a back-edge
        // (run_pipeline's #6463 filter), and every alias binding read from
        // it (`export const x = internal.x`) captures undefined. `deps` is
        // built in source order (imports first, then re-export sources), so
        // simply not sorting preserves the ESM visit order.
        if let Some(module_deps) = deps.get(path) {
            for dep in module_deps {
                dfs_visit(dep, deps, path_to_name, visited, visiting, sorted);
            }
        }

        visiting.remove(path);
        visited.insert(path.clone());
        if let Some(name) = path_to_name.get(path) {
            sorted.push(name.clone());
        }
    }

    // #6463 follow-up: root the DFS at the ENTRY module's imports, in
    // declaration order — the same place node's ESM evaluation starts. The
    // previous alphabetical all-paths iteration produced a valid topological
    // order for DAGs, but whichever alphabetically-early module first reached
    // a cycle decided its break direction, which could invert node's
    // evaluation order for that cycle (see the dep-order comment above).
    // Any module not reachable from the entry through the collected edges
    // (Deferred dynamic-import targets, etc.) is appended afterwards in
    // alphabetical order for determinism.
    if let Some(entry_module) = ctx.native_modules.get(entry_path) {
        for import in &entry_module.imports {
            if import.is_dynamic || import.type_only || import.is_deferred_require {
                continue;
            }
            if let Some(ref resolved) = import.resolved_path {
                let resolved_path = PathBuf::from(resolved);
                if path_to_name.contains_key(&resolved_path) {
                    dfs_visit(
                        &resolved_path,
                        &deps,
                        &path_to_name,
                        &mut visited,
                        &mut visiting,
                        &mut sorted,
                    );
                }
            }
        }
    }

    let mut all_paths: Vec<PathBuf> = path_to_name.keys().cloned().collect();
    all_paths.sort();

    for path in &all_paths {
        dfs_visit(
            path,
            &deps,
            &path_to_name,
            &mut visited,
            &mut visiting,
            &mut sorted,
        );
    }

    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!("\nModule init order ({} modules):", sorted.len());
        for (i, name) in sorted.iter().enumerate() {
            eprintln!("  [{}] {}", i, name);
        }
        eprintln!();
    }

    sorted
}
