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
