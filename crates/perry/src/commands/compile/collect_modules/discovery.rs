//! Module-graph discovery helpers: filesystem walks and the predicates that
//! decide whether a discovered file belongs in the native AOT graph.

use std::fs;
use std::path::{Path, PathBuf};

use super::CompilationContext;

/// Next.js wall 54 (part 2): recursively gather every `*.js` file under `dir`
/// (page/route loaders + turbopack chunks). Symlinks are not followed; errors
/// reading a subdirectory are skipped silently (best-effort discovery).
pub(super) fn collect_js_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_js_files_recursive(&path, out);
        } else if file_type.is_file() && path.extension().and_then(|e| e.to_str()) == Some("js") {
            out.push(path);
        }
    }
}

/// Next.js wall 54 (part 2): true for a module discovered under a standalone
/// bundle's `.next/server/**` tree (page/route/chunk modules loaded by a
/// runtime-computed path). Matched by the `.next` then `server` path-component
/// sequence so it never false-matches a user file merely named `next` or a
/// `node_modules/.next-*` package. Used by init classification (these modules
/// must be eager so they self-register under their path at startup) and topo
/// ordering (chunks before the page loaders that `R.c()` them).
pub(crate) fn is_nextjs_runtime_module(path: &Path) -> bool {
    let comps: Vec<&std::ffi::OsStr> = path.components().map(|c| c.as_os_str()).collect();
    comps
        .windows(2)
        .any(|w| w[0] == std::ffi::OsStr::new(".next") && w[1] == std::ffi::OsStr::new("server"))
}

/// #6769: a statically resolved import must be compiled natively — Perry has
/// no runtime JavaScript engine for it to fall back to. Promoting the FILE
/// alone (rather than its whole package) keeps `perry.compilePackages`
/// meaningful: a runtime-computed load inside an unauthorized package still
/// routes the old way. Both promotion sites — the import walk and the
/// re-export walk — ask this, so the boundary cannot drift between them.
pub(super) fn aot_promotion_is_authorized(resolved_path: &Path, ctx: &CompilationContext) -> bool {
    super::super::audit_manifest::package_name_for_path(&resolved_path.to_string_lossy())
        .is_none_or(|package| {
            ctx.compile_packages.contains(&package)
                && super::super::allowlist_matches(&package, &ctx.allow_compile_packages)
        })
}
