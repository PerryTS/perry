//! TS / JS module resolution: import paths, npm packages, file: deps,
//! perry.nativeLibrary / perry.compilePackages package discovery.
//!
//! Tier 2.1 follow-up (v0.5.340) — extracts the entire resolve_import
//! family + npm-package detection helpers + perry workspace root
//! locator from `compile.rs`. ~810 LOC of self-contained module
//! resolution logic. The fns here cover:
//!
//! - `find_perry_workspace_root` — locates the perry repo root via
//!   the executable path + workspace-marker walk (used by
//!   library_search.rs to find bundled .a files).
//! - `has_perry_native_library` / `has_perry_native_module` —
//!   classify an npm package's `perry` config block.
//! - `parse_native_library_manifest` — read the `nativeLibrary`
//!   field of an npm `package.json` into a structured manifest.
//! - `is_in_perry_native_package`, `extract_compile_package_dir`,
//!   `is_in_compile_package` — directory-membership tests for
//!   classifying resolved paths.
//! - `find_node_modules` — walk-up search.
//! - `find_file_dep_in_package_json` — resolve `"foo": "file:../bar"`
//!   shape (issue #209).
//! - `parse_package_specifier`, `resolve_with_extensions`,
//!   `resolve_package_entry`, `resolve_package_source_entry`,
//!   `resolve_exports` — the per-segment resolution logic.
//! - `resolve_import` + `cached_resolve_import` — the public entry
//!   points + cache.
//! - `discover_extension_entries`, `compute_module_prefix` —
//!   supporting helpers.

use anyhow::{anyhow, Result};
use perry_hir::ModuleKind;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::CompilationContext;
#[cfg(test)]
use super::{NativeBackend, NativeLibraryManifest};

mod native_library;
mod tsconfig_paths;
pub(crate) use native_library::validate_native_library_manifest_value;
pub(super) use native_library::{
    ergonomic_export_alias, has_perry_native_library, has_perry_native_module,
    parse_native_library_manifest, validate_abi_version,
};
#[cfg(test)]
use native_library::{split_module_spec, PERRY_FFI_ABI_VERSION};

/// True when `dir` is the root of the Perry workspace (carries the
/// marker crates the auto-optimize relink reaches for).
fn is_perry_workspace_root(dir: &Path) -> bool {
    dir.join("crates/perry-runtime").is_dir() && dir.join("crates/perry-ui-geisterhand").is_dir()
}

/// Locate the workspace root by walking up from the perry executable.
///
/// A `cargo`/dev install typically exposes `perry` as a **symlink**
/// (e.g. `~/.cargo/bin/perry -> .../target/release/perry`).
/// `std::env::current_exe()` returns that symlink path on macOS, so the
/// `../../` walk would otherwise climb `~/.cargo/bin → ~/.cargo → ~` and
/// never reach the workspace — silently dropping the per-feature
/// `perry-ext-*` libs at link time (issue #2531; same class as #846,
/// whose auto-optimize fix the symlink defeated). Canonicalize the exe
/// first so the walk starts from the real `target/release/perry`.
fn workspace_root_from_exe(exe: &Path) -> Option<PathBuf> {
    let exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let dir = exe.parent()?;
    // Binary in target/release/ → workspace is ../../
    for ancestor in [
        dir.to_path_buf(),
        dir.join(".."),
        dir.join("../.."),
        dir.join("../../.."),
    ] {
        // A missing/uncanonicalizable ancestor must be skipped, not abort
        // the whole search — otherwise the cwd fallback below never runs.
        if let Ok(candidate) = std::fs::canonicalize(&ancestor) {
            if is_perry_workspace_root(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Find the Perry workspace root by searching upward from the executable location.
pub fn find_perry_workspace_root() -> Option<PathBuf> {
    // Explicit override: npm/homebrew installs place the perry binary
    // outside the workspace, so neither the exe walk nor the cwd walk
    // below can ever find the source tree — auto-optimize then silently
    // falls back to the prebuilt full-feature runtime/stdlib and every
    // binary ships the whole stdlib (sqlite, crypto, tokio, …). Users
    // who keep a workspace checkout can point at it explicitly.
    if let Ok(root) = std::env::var("PERRY_WORKSPACE_ROOT") {
        let path = PathBuf::from(root);
        if is_perry_workspace_root(&path) {
            return Some(path);
        }
        eprintln!(
            "warning: PERRY_WORKSPACE_ROOT is set but does not look like a \
             Perry workspace (missing crates/perry-runtime); ignoring it"
        );
    }
    // First try: relative to the perry executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = workspace_root_from_exe(&exe) {
            return Some(root);
        }
    }
    // Second try: current working directory or its ancestors
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if is_perry_workspace_root(dir) {
                return Some(dir.to_path_buf());
            }
            dir = dir.parent()?;
        }
    }
    None
}

#[cfg(test)]
mod bun_store_tests;
#[cfg(test)]
mod tests;

/// Packages that Perry provides built-in native extensions for.
/// These must never be loaded into V8 — Perry's codegen intercepts all imports
/// from these packages and replaces them with native calls.
const PERRY_NATIVE_EXTENSION_PACKAGES: &[&str] = &["ioredis", "ethers", "mysql2", "ws", "dotenv"];

/// Check if a file path is inside a Perry native extension package (has built-in stdlib support)
/// or a package that has perry.nativeLibrary in its package.json.
pub(super) fn is_in_perry_native_package(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    // Check hardcoded native extension packages first (fast path)
    for pkg_name in PERRY_NATIVE_EXTENSION_PACKAGES {
        let needle_slash = format!("node_modules/{}/", pkg_name);
        let needle_end = format!("node_modules/{}", pkg_name);
        if path_str.contains(&needle_slash) || path_str.ends_with(&needle_end) {
            return true;
        }
    }
    // Fall back to package.json perry.nativeLibrary check
    let mut current = path.parent();
    while let Some(dir) = current {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            return has_perry_native_library(dir);
        }
        // Stop at node_modules boundary
        if dir
            .file_name()
            .map(|n| n == "node_modules")
            .unwrap_or(false)
        {
            break;
        }
        current = dir.parent();
    }
    false
}

/// Extract the package directory from a resolved path for a given package name.
/// E.g., for path "/project/node_modules/@noble/curves/node_modules/@noble/hashes/src/sha256.ts"
/// and package_name "@noble/hashes", returns "/project/node_modules/@noble/curves/node_modules/@noble/hashes"
pub(super) fn extract_compile_package_dir(
    resolved_path: &Path,
    package_name: &str,
) -> Option<PathBuf> {
    resolved_path
        .ancestors()
        .find(|candidate| is_compile_package_dir(candidate, package_name))
        .map(Path::to_path_buf)
}

/// Check if a file path is inside a package listed in compile_packages
pub(super) fn is_in_compile_package(path: &Path, compile_packages: &HashSet<String>) -> bool {
    compile_packages.iter().any(|pkg_name| {
        path.ancestors()
            .any(|candidate| is_compile_package_dir(candidate, pkg_name))
    })
}

fn is_compile_package_dir(candidate: &Path, package_name: &str) -> bool {
    let parts: Vec<&str> = package_name.split('/').collect();
    match parts.as_slice() {
        [name] => {
            candidate.file_name().is_some_and(|part| part == *name)
                && candidate
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|part| part == "node_modules")
        }
        [scope, name] => {
            candidate.file_name().is_some_and(|part| part == *name)
                && candidate
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|part| part == *scope)
                && candidate
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .is_some_and(|part| part == "node_modules")
        }
        _ => false,
    }
}

/// Enumerate every installed package name reachable from `project_root`'s
/// `node_modules` tree (the nearest one walking up, plus any nested
/// `node_modules` directories for transitive deps npm chose not to hoist).
///
/// Used by #3527 to materialize the `"*"` / `"@scope/*"` wildcard entries in
/// `perry.compilePackages` into concrete package names. The routing
/// predicates (`is_in_compile_package`, the HIR `COMPILE_PACKAGES_OVERRIDE`,
/// and the many `compile_packages.contains(name)` sites) all match exact
/// package names, so the wildcard has to be expanded before routing or it is a
/// silent no-op. Returns scoped names as `@scope/name`.
pub(super) fn enumerate_installed_packages(project_root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(nm) = find_node_modules(project_root) {
        collect_packages_in_node_modules(&nm, &mut out);
    }
    // #5914: bun's "flat"/isolated linker layout keeps non-hoisted transitive
    // dependencies solely inside `node_modules/.bun/<pkg>@<version>/node_modules/<pkg>`
    // (and the scoped `@scope+pkg@<version>` variant), with no corresponding
    // top-level `node_modules/<pkg>` symlink — bun only symlinks packages
    // that are direct dependencies of some workspace package into the top
    // level. `collect_packages_in_node_modules` correctly skips `.bun` as a
    // dotdir (it is bun's internal store, not itself a package), which means
    // those transitive-only packages are invisible to the `"*"` /
    // `"@scope/*"` wildcard expansion above.
    //
    // Worse, in a bun workspace/monorepo the `.bun` store typically lives
    // ONLY at the true root, while `find_node_modules` stops at the
    // *nearest* ancestor `node_modules` — a workspace member commonly has
    // its own (bun-created, `.bun`-less) `node_modules` for its first-party
    // sibling-package symlinks, so `nm` above is very often NOT the root and
    // never sees `.bun` at all. Walk every ancestor's `node_modules/.bun`,
    // not just the nearest `node_modules` dir, so a workspace-member
    // `project_root` still finds root-level bun-only transitive deps.
    let mut dir = project_root.to_path_buf();
    loop {
        collect_packages_in_bun_store(&dir.join("node_modules"), &mut out);
        if !dir.pop() {
            break;
        }
    }
    out
}

/// See the `.bun` note on `enumerate_installed_packages`. Each
/// `.bun/<entry>/node_modules` subdirectory has the exact same shape as an
/// ordinary `node_modules` directory, so walk it with the same collector.
/// `node_modules` need not exist (`fs::read_dir` on the derived `.bun` path
/// simply fails and returns).
fn collect_packages_in_bun_store(node_modules: &Path, out: &mut HashSet<String>) {
    let bun_dir = node_modules.join(".bun");
    let entries = match fs::read_dir(&bun_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let nested = entry.path().join("node_modules");
        if nested.is_dir() {
            collect_packages_in_node_modules(&nested, out);
        }
    }
}

/// Walk a single `node_modules` directory, recording each package name and
/// recursing into any nested `node_modules` it contains.
fn collect_packages_in_node_modules(node_modules: &Path, out: &mut HashSet<String>) {
    let entries = match fs::read_dir(node_modules) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip npm bookkeeping dirs (`.bin`, `.cache`, `.package-lock.json`).
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if name.starts_with('@') {
            // Scope directory: each child is a `@scope/pkg`.
            if let Ok(scoped) = fs::read_dir(&path) {
                for sub in scoped.flatten() {
                    let sub_name = sub.file_name();
                    let sub_name = sub_name.to_string_lossy();
                    if sub_name.starts_with('.') {
                        continue;
                    }
                    let sub_path = sub.path();
                    if !sub_path.is_dir() {
                        continue;
                    }
                    out.insert(format!("{}/{}", name, sub_name));
                    let nested = sub_path.join("node_modules");
                    if nested.is_dir() {
                        collect_packages_in_node_modules(&nested, out);
                    }
                }
            }
            continue;
        }
        out.insert(name.to_string());
        let nested = path.join("node_modules");
        if nested.is_dir() {
            collect_packages_in_node_modules(&nested, out);
        }
    }
}

/// Find node_modules directory starting from a given path
pub(super) fn find_node_modules(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let node_modules = current.join("node_modules");
        if node_modules.is_dir() {
            return Some(node_modules);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Look up a bare package name in the nearest package.json's `dependencies` /
/// `devDependencies` sections and, if the entry has a `file:` prefix, return the
/// resolved directory path (NOT canonicalized — caller does that).
///
/// This is the fallback used when `node_modules/<pkg>` does not exist (e.g., the
/// user manually removed the symlink, or `npm install` was not re-run after
/// rewriting `package.json` to point at a new `file:` path).  It also covers
/// the "file: dep inside the project root" shape described in #209:
///
///   "bloom": "file:./vendor/bloom/"   ← vendor/bloom may itself be a symlink
///
/// By resolving against the package.json directory (not through the node_modules
/// symlink chain) we arrive at the same canonical target regardless of how many
/// symlink hops npm left behind.
pub(super) fn find_file_dep_in_package_json(start: &Path, package_name: &str) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            if let Ok(content) = fs::read_to_string(&pkg_json) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    for dep_section in &["dependencies", "devDependencies"] {
                        if let Some(deps) = pkg.get(*dep_section).and_then(|d| d.as_object()) {
                            if let Some(dep_val) = deps.get(package_name) {
                                if let Some(dep_str) = dep_val.as_str() {
                                    if let Some(file_path) = dep_str.strip_prefix("file:") {
                                        // Trim trailing slash so dir.join() works cleanly
                                        let resolved = dir.join(file_path.trim_end_matches('/'));
                                        return Some(resolved);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Found a package.json but no matching file: dep for this package.
            // Stop climbing — don't look in ancestor workspaces.
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Parse a package specifier into (package_name, subpath)
pub(super) fn parse_package_specifier(specifier: &str) -> (String, Option<String>) {
    if specifier.starts_with('@') {
        // Scoped package: @scope/package or @scope/package/subpath
        let parts: Vec<&str> = specifier.splitn(3, '/').collect();
        if parts.len() >= 2 {
            let package_name = format!("{}/{}", parts[0], parts[1]);
            let subpath = if parts.len() > 2 {
                Some(parts[2].to_string())
            } else {
                None
            };
            return (package_name, subpath);
        }
    } else {
        // Regular package: package or package/subpath
        let parts: Vec<&str> = specifier.splitn(2, '/').collect();
        let package_name = parts[0].to_string();
        let subpath = if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            None
        };
        return (package_name, subpath);
    }

    (specifier.to_string(), None)
}

/// Try to resolve a path with common extensions
/// Prefers TypeScript source files over JavaScript for native compilation
pub(super) fn resolve_with_extensions(base: &Path) -> Option<PathBuf> {
    // TypeScript extensions to try (in order of preference)
    let ts_extensions = [".ts", ".tsx", ".mts"];
    // JavaScript extensions (fallback)
    let _js_extensions = [".js", ".mjs", ".cjs"];
    // All extensions in order of preference
    let all_extensions = [".ts", ".tsx", ".mts", ".js", ".mjs", ".cjs", ".json"];

    // Check if the path has an explicit JS extension - if so, try TS equivalents first
    if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
        if matches!(ext, "js" | "mjs" | "cjs") {
            // Strip the JS extension and try TS extensions first
            let stem = base.with_extension("");
            for ts_ext in ts_extensions {
                let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                if ts_path.exists() && ts_path.is_file() {
                    return Some(ts_path);
                }
            }
            // If no TS file found, fall back to the original JS file
            if base.exists() && base.is_file() {
                return Some(base.to_path_buf());
            }
        }
    }

    // If it already exists as-is (and not a JS file that we already handled above)
    if base.exists() && base.is_file() {
        // Even if it exists, check for TS version first
        if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "js" | "mjs" | "cjs") {
                let stem = base.with_extension("");
                for ts_ext in ts_extensions {
                    let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
        }
        return Some(base.to_path_buf());
    }

    // Try with extensions in order of preference (TS before JS).
    //
    // Node module resolution APPENDS the extension to the full specifier
    // (`./stream-ops.web` -> `./stream-ops.web.js`); it never strips a dotted
    // segment that isn't a real module extension. `Path::with_extension`
    // REPLACES the last `.foo` segment, so on `stream-ops.web` it produces
    // `stream-ops.js` — which, in Next.js's app-render dir, is the *requiring*
    // module itself (`stream-ops.js` requires `./stream-ops.web`). Returning it
    // makes the module self-require and its re-export getters recurse forever
    // (`exports.chainStreams` -> `self.chainStreams` -> ... stack overflow).
    //
    // So: always try the APPEND form first. Only fall back to the REPLACE form
    // when the specifier already ends in a recognized module extension — that
    // path exists purely for Perry's TS-over-JS preference (`./foo.js` whose
    // `.js` was pruned but `./foo.ts` is present), never to swap an arbitrary
    // filename segment like `.web`.
    let base_ext_is_module = base
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e,
                "js" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "json" | "node"
            )
        })
        .unwrap_or(false);
    let path_str = base.to_string_lossy().to_string();
    for ext in all_extensions {
        // APPEND: `./stream-ops.web` + `.js` -> `./stream-ops.web.js`.
        let appended = PathBuf::from(format!("{}{}", path_str, ext));
        if appended.exists() && appended.is_file() {
            // If we landed on a JS file, prefer a co-located TS source.
            if matches!(ext, ".js" | ".mjs" | ".cjs") {
                for ts_ext in ts_extensions {
                    let ts_path = PathBuf::from(format!("{}{}", path_str, ts_ext));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
            return Some(appended);
        }

        // REPLACE: only safe when the specifier already carries a real module
        // extension (e.g. `./foo.js` -> `./foo.ts`). Skipped for `.web`-style
        // dotted filenames so we never resolve to a sibling module.
        if base_ext_is_module {
            let replaced = base.with_extension(ext.trim_start_matches('.'));
            if replaced.exists() && replaced.is_file() {
                return Some(replaced);
            }
        }
    }

    // Try index files in directory
    if base.is_dir() {
        for ext in all_extensions {
            let index = base.join(format!("index{}", ext));
            if index.exists() {
                return Some(index);
            }
        }
    }

    None
}

/// Resolve package.json entry point
pub(super) fn resolve_package_entry(package_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    let package_json = package_dir.join("package.json");
    if !package_json.exists() {
        // Fall back to index.js
        return resolve_with_extensions(&package_dir.join("index"));
    }

    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try "exports" field first (modern packages), for both main and subpaths
    let export_key = if let Some(sub) = subpath {
        format!("./{}", sub)
    } else {
        ".".to_string()
    };

    if let Some(exports) = pkg.get("exports") {
        // Try every condition branch in priority order and take the first
        // target that exists on disk. A single-winner pick breaks under
        // Next.js standalone output: its file tracing prunes the package
        // files the build didn't load, so `@swc/helpers`' `import` target
        // (`esm/*.js`) is absent while the `default` target (`cjs/*.cjs`)
        // is present — Node resolves the latter at require time.
        let candidates = resolve_exports_candidates(exports, &export_key);
        for entry in &candidates {
            let entry_path = package_dir.join(entry);
            if entry_path.exists() {
                return Some(entry_path);
            }
        }
        // Per Node's resolution algorithm, an applicable `"exports"` entry takes
        // precedence over the legacy `"module"`/`"main"` fields (#5237). When
        // `exports` defines this specifier, it is authoritative: never fall
        // back to `module`/`main` for it, even if no candidate was found on
        // disk above. Falling through mis-resolved e.g. `y18n` (a `yargs` dep)
        // to its `"module"` target `./build/lib/index.js` — a named-export-only
        // file with no `default` — instead of the `exports.import` target
        // `./index.mjs`. We only return early when `exports` actually produced
        // a candidate; an empty list means `exports` doesn't cover this
        // specifier, so the legacy-field fallback below still applies.
        if let Some(first) = candidates.first() {
            return resolve_with_extensions(&package_dir.join(first));
        }
    }

    // If there's a subpath and exports didn't match, resolve it directly
    if let Some(sub) = subpath {
        let subpath_resolved = package_dir.join(sub);
        return resolve_with_extensions(&subpath_resolved);
    }

    // Try "types" or "typings" field for TypeScript
    for field in ["types", "typings"] {
        if let Some(types_path) = pkg.get(field).and_then(|v| v.as_str()) {
            // Look for corresponding .ts file
            let types_file = package_dir.join(types_path);
            let ts_file = types_file.with_extension("ts");
            // Skip .d.ts declaration files - they're type-only, not real source
            if ts_file.exists() && !ts_file.to_string_lossy().ends_with(".d.ts") {
                return Some(ts_file);
            }
        }
    }

    // Try "module" field (ESM)
    if let Some(module) = pkg.get("module").and_then(|v| v.as_str()) {
        let module_path = package_dir.join(module);
        if module_path.exists() {
            return Some(module_path);
        }
    }

    // Try "main" field (CommonJS)
    if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
        let main_path = package_dir.join(main);
        return resolve_with_extensions(&main_path);
    }

    // Fall back to index files
    resolve_with_extensions(&package_dir.join("index"))
}

/// Resolve package entry preferring TypeScript source over compiled JS output.
/// Used for compile_packages where we want to compile from TS source, not bundled JS.
pub(super) fn resolve_package_source_entry(
    package_dir: &Path,
    subpath: Option<&str>,
) -> Option<PathBuf> {
    // For subpaths, try src/<subpath>.ts
    if let Some(sub) = subpath {
        let src_path = package_dir.join("src").join(sub);
        if let Some(resolved) = resolve_with_extensions(&src_path) {
            if !is_js_file(&resolved) {
                return Some(resolved);
            }
        }
    }

    // Try src/index.ts (most common TS source entry)
    let src_index = package_dir.join("src").join("index");
    if let Some(resolved) = resolve_with_extensions(&src_index) {
        if !is_js_file(&resolved) {
            return Some(resolved);
        }
    }

    // Try using normal entry resolution but prefer TS over JS
    let normal_entry = resolve_package_entry(package_dir, subpath)?;
    if is_js_file(&normal_entry) {
        // Try .ts equivalent of the .js entry
        let ts_path = normal_entry.with_extension("ts");
        if ts_path.exists() {
            return Some(ts_path);
        }
        // Check src/ directory mirror of lib/ or dist/ path
        if let Ok(rel) = normal_entry.strip_prefix(package_dir) {
            let rel_str = rel.to_string_lossy();
            if rel_str.starts_with("lib") || rel_str.starts_with("dist") {
                let stripped = if rel_str.starts_with("lib") {
                    rel.strip_prefix("lib")
                } else {
                    rel.strip_prefix("dist")
                };
                if let Ok(rest) = stripped {
                    let src_equiv = package_dir.join("src").join(rest).with_extension("ts");
                    if src_equiv.exists() {
                        return Some(src_equiv);
                    }
                }
            }
        }
    }

    None
}

/// Resolve exports field from package.json
fn resolve_exports_with_conditions(
    exports: &serde_json::Value,
    subpath: &str,
    conditions: &[&str],
) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        // Node "exports" fallback arrays: return the first element that resolves.
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| resolve_exports_with_conditions(item, subpath, conditions)),
        serde_json::Value::Object(map) => {
            // Try the specific subpath first
            if let Some(entry) = map.get(subpath) {
                return resolve_exports_with_conditions(entry, subpath, conditions);
            }

            // Try wildcard patterns (e.g., "./*" -> "./src/*.ts")
            for (key, value) in map.iter() {
                if key.contains('*') {
                    // Convert "./*" to a prefix/suffix match
                    let parts: Vec<&str> = key.splitn(2, '*').collect();
                    if parts.len() == 2 {
                        let prefix = parts[0];
                        let suffix = parts[1];
                        if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                            let matched = &subpath[prefix.len()..subpath.len() - suffix.len()];
                            if let Some(template) =
                                resolve_exports_with_conditions(value, subpath, conditions)
                            {
                                return Some(template.replace('*', matched));
                            }
                        }
                    }
                }
            }

            // Try common conditions (for both main entry and subpath entries)
            // This handles the case where we've matched a subpath and now need to resolve the conditions.
            // "perry" is checked first so packages can ship a TypeScript source entry
            // intended for Perry compilation alongside a pre-built JS entry for Node/Bun.
            for condition in conditions {
                if let Some(entry) = map.get(*condition) {
                    return resolve_exports_with_conditions(entry, subpath, conditions);
                }
            }

            None
        }
        _ => None,
    }
}

/// Resolve exports field from package.json for executable module entries.
///
/// `node` is ranked ABOVE `default` (and above `import`/`module`) because perry
/// compiles a Node target: a package whose `exports` offers a `{ node, default }`
/// conditional pair ships its full Node API under `node` and a reduced
/// browser/edge build under `default`. Picking `default` drops Node-only
/// exports — e.g. `unicorn-magic` exposes `toPath`/`traversePathUp` only in its
/// `node` entry (`./node.js`); resolving `./default.js` left them undefined, so
/// `npm-run-path` (→ execa) failed to LINK (`undefined symbol
/// perry_fn_…unicorn_magic…__toPath`). Matches the `resolve_subpath_import`
/// condition order (chalk's `#supports-color` `{ node, default }`); the two
/// resolvers must agree.
pub(super) fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    resolve_exports_with_conditions(
        exports,
        subpath,
        &["perry", "node", "import", "module", "default", "require"],
    )
}

/// Node subpath imports (#5039): resolve a `#`-prefixed specifier through the
/// importing package's own `package.json` `"imports"` map
/// (https://nodejs.org/api/packages.html#imports). chalk 5 loads its vendored
/// dependencies this way (`import ansiStyles from '#ansi-styles'` →
/// `./source/vendor/ansi-styles/index.js`), so without this every compiled
/// chalk style table came up empty. The map shares the `exports` value shape
/// (string / conditional object / `*` patterns), so the same resolver is
/// reused — with `node` ranked above `default` so conditional pairs like
/// chalk's `#supports-color` `{ node, default: browser }` pick the node build
/// for native compilation. Per Node's package-scope rule, only the NEAREST
/// `package.json` up from the importer is consulted.
fn resolve_subpath_import(import_source: &str, importer_path: &Path) -> Option<PathBuf> {
    let mut dir = importer_path.parent();
    while let Some(d) = dir {
        let pkg_json = d.join("package.json");
        if pkg_json.is_file() {
            let content = std::fs::read_to_string(&pkg_json).ok()?;
            let json: serde_json::Value = serde_json::from_str(&content).ok()?;
            let target = resolve_exports_with_conditions(
                json.get("imports")?,
                import_source,
                &["perry", "node", "import", "module", "default", "require"],
            )?;
            let base = d.join(target.trim_start_matches("./"));
            return resolve_with_extensions(&base)
                .and_then(|p| p.canonicalize().ok())
                .or_else(|| base.canonicalize().ok());
        }
        dir = d.parent();
    }
    None
}

/// Like [`resolve_exports`], but returns EVERY condition branch's resolution
/// in priority order instead of only the first. Callers that check disk
/// existence (`resolve_package_entry`) walk the list so a pruned target
/// (Next.js standalone file tracing) falls through to the next condition.
pub(super) fn resolve_exports_candidates(
    exports: &serde_json::Value,
    subpath: &str,
) -> Vec<String> {
    // `node` ranked above `default` (perry compiles a Node target): a
    // `{ node, default }` conditional pair must resolve the Node build, not the
    // reduced browser/edge `default`. Candidates are collected in this order
    // and the caller picks the first that exists on disk, so listing `node`
    // first makes the Node entry win. (unicorn-magic exposes toPath/
    // traversePathUp only in `./node.js`; resolving `./default.js` left them
    // undefined → npm-run-path → execa LINK failure.) Mirrors `resolve_exports`
    // / `resolve_subpath_import`.
    const CONDITIONS: &[&str] = &["perry", "node", "import", "module", "default", "require"];
    fn collect(value: &serde_json::Value, subpath: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) if !out.contains(s) => {
                out.push(s.clone());
            }
            // Node "exports" fallback arrays (e.g. y18n's
            // `[{ "import": "./index.mjs", "require": "./build/index.cjs" }, "./build/index.cjs"]`).
            // Each element is tried in order; we gather every resolution so the
            // disk-existence walk in `resolve_package_entry` can pick the first
            // that is present.
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, subpath, out);
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(entry) = map.get(subpath) {
                    collect(entry, subpath, out);
                    return;
                }
                for (key, entry) in map.iter() {
                    if key.contains('*') {
                        let parts: Vec<&str> = key.splitn(2, '*').collect();
                        if parts.len() == 2 {
                            let (prefix, suffix) = (parts[0], parts[1]);
                            if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                                let matched = &subpath[prefix.len()..subpath.len() - suffix.len()];
                                let mut templates = Vec::new();
                                collect(entry, subpath, &mut templates);
                                for template in templates {
                                    let resolved = template.replace('*', matched);
                                    if !out.contains(&resolved) {
                                        out.push(resolved);
                                    }
                                }
                            }
                        }
                    }
                }
                for condition in CONDITIONS {
                    if let Some(entry) = map.get(*condition) {
                        collect(entry, subpath, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(exports, subpath, &mut out);
    out
}

fn canonical_existing_declaration(path: PathBuf) -> Option<PathBuf> {
    if path.exists() && is_declaration_file(&path) {
        Some(path.canonicalize().unwrap_or(path))
    } else {
        None
    }
}

fn declaration_sidecar_for_implementation(implementation_path: &Path) -> Option<PathBuf> {
    let ext = implementation_path.extension().and_then(|e| e.to_str())?;
    let candidates: &[&str] = match ext {
        "js" => &["d.ts"],
        "mjs" => &["d.mts", "d.ts"],
        "cjs" => &["d.cts", "d.ts"],
        _ => &[],
    };
    for candidate_ext in candidates {
        if let Some(sidecar) =
            canonical_existing_declaration(implementation_path.with_extension(candidate_ext))
        {
            return Some(sidecar);
        }
    }
    None
}

fn package_dir_for_resolved_path(resolved_path: &Path, package_name: &str) -> Option<PathBuf> {
    if let Some(dir) = extract_compile_package_dir(resolved_path, package_name) {
        return Some(dir);
    }

    let mut current = resolved_path.parent();
    while let Some(dir) = current {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            if let Ok(content) = fs::read_to_string(&pkg_json) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    if pkg.get("name").and_then(|v| v.as_str()) == Some(package_name) {
                        return Some(dir.to_path_buf());
                    }
                }
            }
        }
        current = dir.parent();
    }

    None
}

pub(super) fn resolve_package_declaration_entry(
    package_dir: &Path,
    subpath: Option<&str>,
    implementation_path: Option<&Path>,
) -> Option<PathBuf> {
    let package_json = package_dir.join("package.json");
    if package_json.exists() {
        let pkg = fs::read_to_string(&package_json)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());

        if let Some(pkg) = pkg {
            let export_key = if let Some(sub) = subpath {
                format!("./{}", sub)
            } else {
                ".".to_string()
            };

            if let Some(exports) = pkg.get("exports") {
                if let Some(entry) =
                    resolve_exports_with_conditions(exports, &export_key, &["types", "typings"])
                {
                    if let Some(path) = canonical_existing_declaration(package_dir.join(entry)) {
                        return Some(path);
                    }
                }
            }

            if subpath.is_none() {
                for field in ["types", "typings"] {
                    if let Some(types_path) = pkg.get(field).and_then(|v| v.as_str()) {
                        if let Some(path) =
                            canonical_existing_declaration(package_dir.join(types_path))
                        {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    implementation_path.and_then(declaration_sidecar_for_implementation)
}

pub(super) fn declaration_sidecar_for_resolved_import(
    import_source: &str,
    resolved_path: &Path,
) -> Option<PathBuf> {
    if is_declaration_file(resolved_path) {
        return canonical_existing_declaration(resolved_path.to_path_buf());
    }

    if !(is_relative_specifier(import_source) || import_source.starts_with('/')) {
        let (package_name, subpath) = parse_package_specifier(import_source);
        if let Some(package_dir) = package_dir_for_resolved_path(resolved_path, &package_name) {
            if let Some(sidecar) = resolve_package_declaration_entry(
                &package_dir,
                subpath.as_deref(),
                Some(resolved_path),
            ) {
                return Some(sidecar);
            }
        }
    }

    declaration_sidecar_for_implementation(resolved_path)
}

/// Determine if a file is a JavaScript file (not TypeScript)
pub(super) fn is_js_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "js" | "mjs" | "cjs")
    } else {
        false
    }
}

/// #5223: Recognized text-asset extensions. An import resolving to one of these
/// is loaded as a string (its raw contents become the module's default export)
/// rather than TS-parsed. `.wasm` is intentionally excluded (out of scope).
pub(super) fn is_recognized_text_asset(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "txt"
                | "sql"
                | "md"
                | "html"
                | "htm"
                | "css"
                | "graphql"
                | "gql"
                | "glsl"
                | "vert"
                | "frag"
        )
    } else {
        false
    }
}

/// Determine if a file is a TypeScript declaration file (.d.ts)
pub(super) fn is_declaration_file(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}

/// Determine if a file is a TypeScript file (but not a declaration file)
pub(super) fn is_ts_file(path: &Path) -> bool {
    if is_declaration_file(path) {
        return false;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "ts" | "tsx")
    } else {
        false
    }
}

pub(super) fn resolve_relative_import_path(
    import_source: &str,
    importer_path: &Path,
) -> Option<PathBuf> {
    resolve_relative_import_paths(import_source, importer_path)
        .map(|resolved| resolved.canonical_path)
}

pub(super) struct ResolvedPath {
    pub source_path: PathBuf,
    pub canonical_path: PathBuf,
}

pub(super) fn resolve_relative_import_paths(
    import_source: &str,
    importer_path: &Path,
) -> Option<ResolvedPath> {
    if !is_relative_specifier(import_source) {
        return None;
    }
    let parent = importer_path.parent()?;
    let resolved = parent.join(import_source);
    // Source import specifiers are resolved against the path as written by the
    // program. If that path contains a symlinked component such as /tmp, asking
    // the filesystem about "a/../b" can follow the symlink before applying ".."
    // and accidentally probe the canonical sibling.
    let lexical = normalize_path_lexically(&resolved);
    let source_path = resolve_with_extensions(&lexical).or_else(|| {
        if lexical == resolved {
            None
        } else {
            resolve_with_extensions(&resolved)
        }
    })?;
    let canonical_path = source_path.canonicalize().ok()?;
    Some(ResolvedPath {
        source_path,
        canonical_path,
    })
}

pub(super) fn resolve_absolute_import_paths(import_source: &str) -> Option<ResolvedPath> {
    if !import_source.starts_with('/') {
        return None;
    }
    let source_path = resolve_with_extensions(&PathBuf::from(import_source))?;
    let canonical_path = source_path.canonicalize().ok()?;
    Some(ResolvedPath {
        source_path,
        canonical_path,
    })
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                } else {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// True for ECMAScript relative-import specifiers. Besides the obvious `./x`
/// and `../x`, the bare `"."` and `".."` are also relative — they resolve to
/// the current / parent **directory**'s `index` file. `@tanstack/table-core`'s
/// source uses `import { _getVisibleLeafColumns } from '..'` (the package
/// barrel); without matching `".."` here it fell through to bare-package
/// resolution, `import.resolved_path` never matched the index module, and every
/// name imported through it lowered to an unresolved raw extern symbol → link
/// failure (`__getVisibleLeafColumns`). Refs #5141.
pub(super) fn is_relative_specifier(import_source: &str) -> bool {
    import_source.starts_with("./")
        || import_source.starts_with("../")
        || import_source == "."
        || import_source == ".."
}

/// Resolve an import specifier to a file path
pub(super) fn resolve_import(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &HashMap<String, PathBuf>,
) -> Option<(PathBuf, ModuleKind)> {
    // Check if it's a native Rust stdlib module. Refs #665: when the user has
    // explicitly opted the package into `perry.compilePackages`, they want
    // their `node_modules` copy compiled from source (cjs_wrap + native
    // codegen), not the built-in Rust FFI binding — which for some packages
    // (e.g. `rate-limiter-flexible`'s `perry-ext-ratelimit`) is incomplete.
    // The opt-in is package-scoped: bare `rate-limiter-flexible` and any
    // subpath under it both fall through to file resolution.
    let (native_check_pkg, _) = parse_package_specifier(import_source);
    if perry_hir::is_native_module(import_source) && !compile_packages.contains(&native_check_pkg) {
        return None; // Native modules are handled by stdlib, not file imports
    }

    // Node subpath imports (`#…`, #5039) resolve through the importing
    // package's own `"imports"` map and then classify exactly like a relative
    // import to the mapped file.
    let subpath_import_target = if import_source.starts_with('#') {
        match resolve_subpath_import(import_source, importer_path) {
            Some(canonical) => Some(canonical),
            None => return None,
        }
    } else {
        None
    };

    // Handle relative imports (./ or ../, plus bare "." / ".." directory imports)
    if is_relative_specifier(import_source) || subpath_import_target.is_some() {
        if let Some(canonical) = subpath_import_target
            .or_else(|| resolve_relative_import_path(import_source, importer_path))
        {
            // Refs #486: a relative `import './foo.js'` from inside a compile
            // package must classify as NativeCompiled even when the resolved
            // file lives outside the literal `node_modules/<pkg>/` substring
            // — `file:./lib3` deps and symlinked package roots both canonicalize
            // away from `node_modules`, but their files are still part of the
            // compile-package compile scope. Without this, re-exports inside
            // such packages (e.g. `lib3/index.js` doing `export { C } from
            // './c.js'`) silently fall through to ModuleKind::Interpreted, the
            // dependent file never enters `ctx.native_modules`, and importing
            // modules see `imported_classes=[]` for symbols re-exported from it.
            let in_compile_pkg = is_in_compile_package(&canonical, compile_packages)
                || compile_package_dirs.values().any(|dir| {
                    if canonical.starts_with(dir) {
                        let relative = canonical.strip_prefix(dir).unwrap_or(canonical.as_path());
                        !relative.to_string_lossy().contains("node_modules/")
                    } else {
                        false
                    }
                });
            // #1721 / #668: a *user* `.js` (outside node_modules) compiles
            // natively now — mirrors collect_modules' `should_use_js_runtime`.
            // Its import edge MUST be NativeCompiled so the importer wires
            // `perry_fn_<prefix>__*` symbols; leaving it Interpreted routes to
            // the (removed, post-#1696) V8 bridge and the default/named export
            // symbols never link.
            let in_node_modules = canonical.to_string_lossy().contains("node_modules");
            let kind = if is_js_file(&canonical) && !in_compile_pkg && in_node_modules {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((canonical, kind));
        }
        return None;
    }

    // Handle absolute paths
    if import_source.starts_with('/') {
        let resolved = PathBuf::from(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let canonical = path.canonicalize().ok()?;
            // #1721: same node_modules-gated rule as relative imports.
            let in_node_modules = canonical.to_string_lossy().contains("node_modules");
            let kind = if is_js_file(&canonical) && in_node_modules {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((canonical, kind));
        }
        return None;
    }

    // Handle node_modules (bare specifiers)
    let (package_name, subpath) = parse_package_specifier(import_source);

    // For compile_packages, search project root first to prefer ESM versions
    // over nested CJS copies (e.g., @solana/web3.js/node_modules/bs58 is CJS,
    // but the top-level node_modules/bs58 has ESM support)
    let search_paths = if compile_packages.contains(&package_name) {
        [Some(project_root), importer_path.parent()]
    } else {
        [importer_path.parent(), Some(project_root)]
    };

    for start in search_paths.iter().flatten() {
        if let Some(node_modules) = find_node_modules(start) {
            let package_dir = node_modules.join(&package_name);
            if package_dir.is_dir() {
                if let Some(entry) = resolve_package_entry(&package_dir, subpath.as_deref()) {
                    // Packages with perry.nativeLibrary are compiled natively (Rust FFI)
                    if has_perry_native_library(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages with perry.nativeModule: true contain Perry-compatible
                    // TypeScript that must be compiled natively (e.g. perry-react).
                    if has_perry_native_module(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages listed in perry.compilePackages are compiled natively
                    if compile_packages.contains(&package_name) {
                        // Deduplicate: if we've already resolved this package from a
                        // different node_modules location, use the first-found directory
                        // to avoid duplicate symbols from identical package copies
                        let effective_dir = compile_package_dirs
                            .get(&package_name)
                            .unwrap_or(&package_dir);
                        // Prefer TypeScript source over compiled JS
                        if let Some(src_entry) =
                            resolve_package_source_entry(effective_dir, subpath.as_deref())
                        {
                            return Some((
                                src_entry.canonicalize().ok()?,
                                ModuleKind::NativeCompiled,
                            ));
                        }
                        // Fall back to normal resolution but still mark as NativeCompiled
                        if let Some(fallback_entry) =
                            resolve_package_entry(effective_dir, subpath.as_deref())
                        {
                            return Some((
                                fallback_entry.canonicalize().ok()?,
                                ModuleKind::NativeCompiled,
                            ));
                        }
                        // If effective_dir failed (shouldn't happen), try the local dir
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // For other node_modules packages, classify by file
                    // extension. `.ts` / `.tsx` sources are compiled natively.
                    // `.js` / `.mjs` / `.cjs` and other shapes stay Interpreted;
                    // since runtime-JS (V8) support was removed, reaching one of
                    // these is a hard error surfaced by the V8-free gate after
                    // module collection.
                    let canonical = entry.canonicalize().ok()?;
                    let kind = if is_ts_file(&canonical) {
                        ModuleKind::NativeCompiled
                    } else {
                        ModuleKind::Interpreted
                    };
                    return Some((canonical, kind));
                }
            }
        }
    }

    // Fallback: look for a `file:` entry in the nearest package.json.
    //
    // Handles two failure modes that the node_modules walk above cannot catch:
    //
    //   1. `node_modules/<pkg>` was removed (or npm install was not re-run after
    //      changing package.json).  The manual repro in #209 hits this directly.
    //
    //   2. `node_modules/<pkg>` exists but points *inside* the project root via an
    //      intermediate symlink (e.g. `node_modules/bloom -> ../vendor/bloom` where
    //      `vendor/bloom` is itself a symlink or a real directory cloned by CI).
    //      In that case the canonical path resolves to a path like
    //      `/project/vendor/bloom/index.ts` — which is inside the project root but
    //      outside any `node_modules/` component — so the `is_in_node_modules`
    //      string check returns false and downstream classify-as-Interpreted guards
    //      can misfire for JS files.  Resolving directly from `package.json` gives
    //      us the same canonical target while keeping `package_dir` pointing at the
    //      real package root (with its perry.nativeLibrary / perry.nativeModule
    //      marker) so `has_perry_native_library` can read it without traversing a
    //      potentially-confusing symlink chain.
    if let Some(file_dep_dir) = find_file_dep_in_package_json(project_root, &package_name) {
        if file_dep_dir.is_dir() {
            if let Some(entry) = resolve_package_entry(&file_dep_dir, subpath.as_deref()) {
                if has_perry_native_library(&file_dep_dir) {
                    return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                }
                if has_perry_native_module(&file_dep_dir) {
                    return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                }
                if compile_packages.contains(&package_name) {
                    if let Some(src_entry) =
                        resolve_package_source_entry(&file_dep_dir, subpath.as_deref())
                    {
                        return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    if let Some(fallback_entry) =
                        resolve_package_entry(&file_dep_dir, subpath.as_deref())
                    {
                        return Some((
                            fallback_entry.canonicalize().ok()?,
                            ModuleKind::NativeCompiled,
                        ));
                    }
                }
                // `.ts`/`.tsx` → NativeCompiled, as the node_modules fallback
                // above. #1721: `file:` deps live outside node_modules (user
                // code), so their `.js` also compiles natively; only genuine
                // node_modules JS stays Interpreted.
                let canonical = entry.canonicalize().ok()?;
                let in_node_modules = canonical.to_string_lossy().contains("node_modules");
                let kind = if is_ts_file(&canonical) || !in_node_modules {
                    ModuleKind::NativeCompiled
                } else {
                    ModuleKind::Interpreted
                };
                return Some((canonical, kind));
            }
        }
    }

    // Final additive fallback (#5214): tsconfig `compilerOptions.paths` /
    // `baseUrl`. Consulted only after relative + package + file: resolution
    // all failed — i.e. exactly the specifiers that would otherwise be
    // "Could not resolve import". A specifier matched here resolves to a real
    // file inside the project, so classify it like a relative/user import:
    // `.ts`/`.tsx` and any user (non-node_modules) file compile natively; only
    // genuine node_modules JS stays Interpreted.
    if let Some(canonical) = tsconfig_paths::resolve_tsconfig_paths(import_source, importer_path) {
        let in_compile_pkg = is_in_compile_package(&canonical, compile_packages)
            || compile_package_dirs
                .values()
                .any(|dir| canonical.starts_with(dir));
        let in_node_modules = canonical.to_string_lossy().contains("node_modules");
        let kind = if is_js_file(&canonical) && !in_compile_pkg && in_node_modules {
            ModuleKind::Interpreted
        } else {
            ModuleKind::NativeCompiled
        };
        return Some((canonical, kind));
    }

    None
}

/// Discover extension entry points from a directory of plugins.
/// Each subdirectory is checked for a package.json with an `openclaw.extensions` array.
/// Returns Vec<(entry_path, plugin_id)> — e.g., ("extensions/telegram/index.ts", "telegram").
pub(super) fn discover_extension_entries(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = Vec::new();

    if !dir.is_dir() {
        return Err(anyhow!(
            "--bundle-extensions path is not a directory: {}",
            dir.display()
        ));
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let subdir = entry.path();
        if !subdir.is_dir() {
            continue;
        }

        let plugin_id = subdir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let pkg_json_path = subdir.join("package.json");
        if pkg_json_path.exists() {
            // Read package.json and look for openclaw.extensions
            let pkg_contents = fs::read_to_string(&pkg_json_path)
                .map_err(|e| anyhow!("Failed to read {}: {}", pkg_json_path.display(), e))?;
            let pkg: serde_json::Value = serde_json::from_str(&pkg_contents)
                .map_err(|e| anyhow!("Failed to parse {}: {}", pkg_json_path.display(), e))?;

            let extensions = pkg
                .get("openclaw")
                .and_then(|oc| oc.get("extensions"))
                .and_then(|ext| ext.as_array());

            if let Some(ext_array) = extensions {
                for ext_entry in ext_array {
                    if let Some(rel_path) = ext_entry.as_str() {
                        let entry_path = subdir.join(rel_path.trim_start_matches("./"));
                        if entry_path.exists() {
                            entries.push((entry_path, plugin_id.clone()));
                        }
                    }
                }
            } else {
                // Fallback: look for index.ts
                let index_path = subdir.join("index.ts");
                if index_path.exists() {
                    entries.push((index_path, plugin_id));
                }
            }
        } else {
            // No package.json — try index.ts directly
            let index_path = subdir.join("index.ts");
            if index_path.exists() {
                entries.push((index_path, plugin_id));
            }
        }
    }

    // Sort for deterministic ordering
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Compute a sanitized module prefix from a resolved path for scoped cross-module symbols
pub(super) fn compute_module_prefix(resolved_path: &str, project_root: &Path) -> String {
    let source_path = PathBuf::from(resolved_path);
    let source_module_name = source_path
        .strip_prefix(project_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            source_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("module")
                .to_string()
        });
    let mut prefix = source_module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
    // LLVM IR identifiers cannot start with a digit. Prefix with `_`
    // if the first character would be one (e.g. `05_fibonacci.ts`).
    if prefix
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        prefix.insert(0, '_');
    }
    prefix
}

/// Cached wrapper around resolve_import to avoid redundant I/O
pub(super) fn cached_resolve_import(
    import_source: &str,
    importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<(PathBuf, ModuleKind)> {
    let importer_dir = importer_path
        .parent()
        .unwrap_or(importer_path)
        .to_path_buf();
    let cache_key = (import_source.to_string(), importer_dir);
    if let Some(cached) = ctx.resolve_cache.get(&cache_key) {
        return cached.clone();
    }
    let result = resolve_import(
        import_source,
        importer_path,
        &ctx.project_root,
        &ctx.compile_packages,
        &ctx.compile_package_dirs,
    );
    ctx.resolve_cache.insert(cache_key, result.clone());
    result
}
