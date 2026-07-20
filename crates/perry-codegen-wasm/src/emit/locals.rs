//! Local/module-let id collection helpers extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move: `collect_module_let_ids`, `resolve_source_module_idx`, `collect_locals`.

use perry_hir::ir::*;
use perry_types::LocalId;
use std::collections::BTreeMap;

/// Recursively scan statements for local variable declarations
/// Walk a module's init statements and assign WASM global indices to top-level Lets.
/// Module-level Lets are then accessible from any function in the same module via
/// the (mod_idx, LocalId) key.
pub(super) fn collect_module_let_ids(
    stmts: &[Stmt],
    mod_idx: usize,
    map: &mut BTreeMap<(usize, LocalId), u32>,
    next_global: &mut u32,
) {
    for stmt in stmts {
        if let Stmt::Let { id, .. } = stmt {
            map.insert((mod_idx, *id), *next_global);
            *next_global += 1;
        }
    }
}

/// Issue #1071: resolve an `Import` to its source module's index in the
/// `modules` vec. The driver populates `import.resolved_path` with an
/// absolute path; `Module.name` is a relative path from the project root
/// (e.g. `theme-src.ts` or `subdir/util.ts`). We match by suffix so the
/// two coordinate systems line up. If `resolved_path` is unset (rare —
/// happens for stdlib imports + a few JSX-runtime synthetic edges) we
/// fall back to suffix-matching `import.source` against module names.
pub(super) fn resolve_source_module_idx(
    modules: &[(String, perry_hir::ir::Module)],
    import: &perry_hir::ir::Import,
    _name_to_idx: &std::collections::HashMap<&str, usize>,
) -> Option<usize> {
    if let Some(ref rp) = import.resolved_path {
        // Match the longest module-name suffix of the resolved absolute path.
        // Modules have names like "theme-src.ts" or "sub/util.ts" and the
        // resolved path looks like "/abs/path/to/project/theme-src.ts".
        let mut best: Option<(usize, usize)> = None;
        for (i, (_, m)) in modules.iter().enumerate() {
            if rp.ends_with(&m.name) {
                let n = m.name.len();
                if best.map(|(_, bn)| n > bn).unwrap_or(true) {
                    best = Some((i, n));
                }
            }
        }
        if let Some((i, _)) = best {
            return Some(i);
        }
    }
    // Fallback: match `import.source` suffix (strip leading "./" / "../").
    let src = import
        .source
        .trim_start_matches("./")
        .trim_start_matches("../");
    let mut best: Option<(usize, usize)> = None;
    for (i, (_, m)) in modules.iter().enumerate() {
        let mn = m.name.as_str();
        // Match "theme-src" against "theme-src.ts" / "theme-src.tsx".
        let stem = mn.rsplit_once('.').map(|(s, _)| s).unwrap_or(mn);
        if stem == src || mn == src {
            let n = mn.len();
            if best.map(|(_, bn)| n > bn).unwrap_or(true) {
                best = Some((i, n));
            }
        }
    }
    best.map(|(i, _)| i)
}

pub(super) fn collect_locals(
    stmts: &[Stmt],
    map: &mut BTreeMap<LocalId, u32>,
    count: &mut u32,
    offset: u32,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, .. } if !map.contains_key(id) => {
                map.insert(*id, offset + *count);
                *count += 1;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_locals(then_branch, map, count, offset);
                if let Some(eb) = else_branch {
                    collect_locals(eb, map, count, offset);
                }
            }
            Stmt::While { body, .. } => {
                collect_locals(body, map, count, offset);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_locals(std::slice::from_ref(init_stmt.as_ref()), map, count, offset);
                }
                collect_locals(body, map, count, offset);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_locals(body, map, count, offset);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        if !map.contains_key(id) {
                            map.insert(*id, offset + *count);
                            *count += 1;
                        }
                    }
                    collect_locals(&c.body, map, count, offset);
                }
                if let Some(f) = finally {
                    collect_locals(f, map, count, offset);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_locals(&case.body, map, count, offset);
                }
            }
            _ => {}
        }
    }
}

/// String-based sibling of `resolve_source_module_idx` for `Export::ReExport
/// { source }` / `Export::ExportAll { source }`, which carry only the module
/// specifier (no resolved path). Same suffix/stem matching as the fallback
/// branch above.
pub(super) fn resolve_module_idx_by_source(
    modules: &[(String, perry_hir::ir::Module)],
    source: &str,
) -> Option<usize> {
    let src = source
        .trim_start_matches("./")
        .trim_start_matches("../")
        .replace('\\', "/");
    // A directory specifier ("./core") resolves to its index module.
    let src_index = format!("{}/index", src);
    let mut best: Option<(usize, usize)> = None;
    for (i, (_, m)) in modules.iter().enumerate() {
        // Module names are project-relative paths with the platform's
        // separators ("engine\src\core\keys.ts" on Windows) — normalize.
        let mn = m.name.replace('\\', "/");
        let stem = mn
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| mn.clone());
        let hit = stem == src
            || mn == src
            || stem.ends_with(&format!("/{}", src))
            || stem.ends_with(&format!("/{}", src_index))
            || stem == src_index;
        if hit {
            let n = mn.len();
            if best.map(|(_, bn)| n > bn).unwrap_or(true) {
                best = Some((i, n));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Is `name` part of module `m`'s own PUBLIC surface — i.e. does it appear
/// in an `export` declaration (named, re-export, exported function, or
/// exported object)? Used to gate the resolve_export_to_* fallbacks so they
/// never hand back a PRIVATE module-local: during `export *` recursion, a
/// private `foo` in an early source must not mask a real exported `foo` in a
/// later one. Non-recursive by design — `export *`-reachable names are
/// resolved by the explicit ExportAll traversal, not the fallback.
pub(super) fn module_exports_name(m: &perry_hir::ir::Module, name: &str) -> bool {
    for e in &m.exports {
        match e {
            perry_hir::ir::Export::Named { exported, .. }
            | perry_hir::ir::Export::ReExport { exported, .. } => {
                if exported == name {
                    return true;
                }
            }
            _ => {}
        }
    }
    m.exported_functions.iter().any(|(n, _)| n == name)
        || m.exported_objects.iter().any(|n| n == name)
}

/// The names a `import * as W from "mod"` namespace should expose: mod's own
/// named/re-exported/function/object exports, plus — recursively — every
/// name re-exported through `export * from "..."`. Deduped; depth-capped.
/// Replaces the old "register every module-level let" fallback, which leaked
/// private locals into the namespace object.
pub(super) fn collect_exported_names(
    modules: &[(String, perry_hir::ir::Module)],
    mod_idx: usize,
    depth: u32,
    out: &mut std::collections::BTreeSet<String>,
) {
    if depth == 0 {
        return;
    }
    let m = &modules[mod_idx].1;
    for e in &m.exports {
        match e {
            perry_hir::ir::Export::Named { exported, .. }
            | perry_hir::ir::Export::ReExport { exported, .. } => {
                out.insert(exported.clone());
            }
            perry_hir::ir::Export::ExportAll { source } => {
                if let Some(si) = resolve_module_idx_by_source(modules, source) {
                    if si != mod_idx {
                        collect_exported_names(modules, si, depth - 1, out);
                    }
                }
            }
            // `export * as ns from "..."` binds the whole namespace under one
            // name; the namespace object itself is not a promoted let we can
            // resolve here, so expose the name (resolution is a no-op) rather
            // than recurse into the source's members.
            perry_hir::ir::Export::NamespaceReExport { name, .. } => {
                out.insert(name.clone());
            }
        }
    }
    for (n, _) in &m.exported_functions {
        out.insert(n.clone());
    }
    for n in &m.exported_objects {
        out.insert(n.clone());
    }
}

/// Resolve module `mod_idx`'s export `name` to a promoted-let wasm global,
/// following re-export chains: `Export::Named` whose local is itself an
/// import binding, `Export::ReExport { source, imported }`, and
/// `Export::ExportAll { source }` star re-exports. Depth-capped — a facade
/// index re-exporting from a sub-index re-exporting from the defining module
/// is the normal library shape (bloom's `Key` is three hops from a consumer).
pub(super) fn resolve_export_to_let(
    modules: &[(String, perry_hir::ir::Module)],
    src_let_names: &[std::collections::HashMap<String, u32>],
    name_to_idx: &std::collections::HashMap<&str, usize>,
    mod_idx: usize,
    name: &str,
    depth: u32,
) -> Option<u32> {
    if depth == 0 {
        return None;
    }
    let m = &modules[mod_idx].1;
    for export in &m.exports {
        match export {
            perry_hir::ir::Export::Named { local, exported } if exported == name => {
                if let Some(&g) = src_let_names[mod_idx].get(local.as_str()) {
                    return Some(g);
                }
                // The exported local may itself be an import binding
                // (`import { Key } from "./core"; export { Key };`).
                for import in &m.imports {
                    if import.type_only {
                        continue;
                    }
                    for spec in &import.specifiers {
                        if let perry_hir::ir::ImportSpecifier::Named {
                            imported,
                            local: il,
                        } = spec
                        {
                            if il == local {
                                if let Some(si) =
                                    resolve_source_module_idx(modules, import, name_to_idx)
                                {
                                    if let Some(g) = resolve_export_to_let(
                                        modules,
                                        src_let_names,
                                        name_to_idx,
                                        si,
                                        imported,
                                        depth - 1,
                                    ) {
                                        return Some(g);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            perry_hir::ir::Export::ReExport {
                source,
                imported,
                exported,
            } if exported == name => {
                if let Some(si) = resolve_module_idx_by_source(modules, source) {
                    if let Some(g) = resolve_export_to_let(
                        modules,
                        src_let_names,
                        name_to_idx,
                        si,
                        imported,
                        depth - 1,
                    ) {
                        return Some(g);
                    }
                }
            }
            _ => {}
        }
    }
    // Star re-exports: the name isn't listed, try every `export * from`.
    for export in &m.exports {
        if let perry_hir::ir::Export::ExportAll { source } = export {
            if let Some(si) = resolve_module_idx_by_source(modules, source) {
                if si != mod_idx {
                    if let Some(g) = resolve_export_to_let(
                        modules,
                        src_let_names,
                        name_to_idx,
                        si,
                        name,
                        depth - 1,
                    ) {
                        return Some(g);
                    }
                }
            }
        }
    }
    // Fall-through: an export registered out-of-band (e.g. an exported
    // object-const) keeps its let by name — but ONLY if the name is actually
    // part of this module's public surface. Returning a private local here
    // would let it mask a genuine export of the same name in a later
    // `export *` source.
    if module_exports_name(m, name) {
        src_let_names[mod_idx].get(name).copied()
    } else {
        None
    }
}

/// Function twin of `resolve_export_to_let`: resolve module `mod_idx`'s
/// export `name` to a compiled function index, following the same re-export
/// shapes. `module_func_maps[i]` maps FuncId → wasm function index for
/// module i.
pub(super) fn resolve_export_to_func(
    modules: &[(String, perry_hir::ir::Module)],
    module_func_maps: &[std::collections::BTreeMap<perry_types::FuncId, u32>],
    name_to_idx: &std::collections::HashMap<&str, usize>,
    mod_idx: usize,
    name: &str,
    depth: u32,
) -> Option<u32> {
    if depth == 0 {
        return None;
    }
    let m = &modules[mod_idx].1;
    let find_local_fn = |local: &str| -> Option<u32> {
        for f in &m.functions {
            if f.name == local {
                if let Some(&idx) = module_func_maps[mod_idx].get(&f.id) {
                    return Some(idx);
                }
            }
        }
        None
    };
    // exported_functions is the authoritative `export function foo` list.
    for (exp_name, fid) in &m.exported_functions {
        if exp_name == name {
            if let Some(&idx) = module_func_maps[mod_idx].get(fid) {
                return Some(idx);
            }
        }
    }
    for export in &m.exports {
        match export {
            perry_hir::ir::Export::Named { local, exported } if exported == name => {
                if let Some(idx) = find_local_fn(local) {
                    return Some(idx);
                }
                for import in &m.imports {
                    if import.type_only {
                        continue;
                    }
                    for spec in &import.specifiers {
                        if let perry_hir::ir::ImportSpecifier::Named {
                            imported,
                            local: il,
                        } = spec
                        {
                            if il == local {
                                if let Some(si) =
                                    resolve_source_module_idx(modules, import, name_to_idx)
                                {
                                    if let Some(idx) = resolve_export_to_func(
                                        modules,
                                        module_func_maps,
                                        name_to_idx,
                                        si,
                                        imported,
                                        depth - 1,
                                    ) {
                                        return Some(idx);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            perry_hir::ir::Export::ReExport {
                source,
                imported,
                exported,
            } if exported == name => {
                if let Some(si) = resolve_module_idx_by_source(modules, source) {
                    if let Some(idx) = resolve_export_to_func(
                        modules,
                        module_func_maps,
                        name_to_idx,
                        si,
                        imported,
                        depth - 1,
                    ) {
                        return Some(idx);
                    }
                }
            }
            _ => {}
        }
    }
    for export in &m.exports {
        if let perry_hir::ir::Export::ExportAll { source } = export {
            if let Some(si) = resolve_module_idx_by_source(modules, source) {
                if si != mod_idx {
                    if let Some(idx) = resolve_export_to_func(
                        modules,
                        module_func_maps,
                        name_to_idx,
                        si,
                        name,
                        depth - 1,
                    ) {
                        return Some(idx);
                    }
                }
            }
        }
    }
    // Fall-through: an out-of-band exported function keeps its name — but
    // only if it's genuinely exported (see resolve_export_to_let).
    if module_exports_name(m, name) {
        find_local_fn(name)
    } else {
        None
    }
}
