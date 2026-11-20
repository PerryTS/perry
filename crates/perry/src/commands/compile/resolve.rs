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
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::CompilationContext;
#[cfg(test)]
use super::{NativeBackend, NativeLibraryManifest};

mod native_library;
mod solid;
pub(super) use solid::resolve_import_with_context;
// pub(crate): the `check --check-deps` dependency checker (commands/deps.rs)
// consults both resolvers so `#` subpath imports and tsconfig-aliased
// specifiers stop reporting false R003 "not found in node_modules" errors.
pub(crate) mod subpath_imports;
pub(crate) mod tsconfig_paths;
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

/// Find workspace source via an override, the executable, its build checkout,
/// or the current directory.
pub fn find_perry_workspace_root() -> Option<PathBuf> {
    // Explicit override: npm/homebrew installs place the perry binary
    // outside the workspace, so neither the exe walk nor the cwd walk
    // below can ever find the source tree — auto-optimize then silently
    // falls back to the prebuilt full-feature runtime/stdlib and every
    // binary ships the whole stdlib (sqlite, crypto, tls, …). Users
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
    workspace_root_from_locations(
        std::env::current_exe().ok().as_deref(),
        std::env::current_dir().ok().as_deref(),
        Path::new(env!("PERRY_BUILD_WORKSPACE_ROOT")),
    )
}

/// The embedded checkout is only a hint: an installed compiler can outlive it,
/// or the directory can be reused for another checkout. Require a Perry
/// workspace at the compiler's version before building libraries from it.
fn workspace_root_from_build_record(root: &Path) -> Option<PathBuf> {
    if !is_perry_workspace_root(root) {
        return None;
    }
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).ok()?).ok()?;
    let version = manifest
        .get("workspace")?
        .get("package")?
        .get("version")?
        .as_str()?;
    if version != env!("CARGO_PKG_VERSION") {
        return None;
    }
    let compiler: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("crates/perry/Cargo.toml")).ok()?).ok()?;
    if compiler.get("package")?.get("name")?.as_str()? != "perry" {
        return None;
    }
    root.canonicalize().ok()
}

fn workspace_root_from_locations(
    exe: Option<&Path>,
    cwd: Option<&Path>,
    build_root: &Path,
) -> Option<PathBuf> {
    if let Some(root) = exe.and_then(workspace_root_from_exe) {
        return Some(root);
    }
    if let Some(root) = workspace_root_from_build_record(build_root) {
        return Some(root);
    }
    if let Some(mut dir) = cwd {
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
mod build_workspace_tests;

#[cfg(test)]
mod bun_store_tests;
#[cfg(test)]
mod declaration_map_source_tests;
#[cfg(test)]
mod exports_subpath_source_tests;
#[cfg(test)]
mod extension_resolution_tests;
#[cfg(test)]
mod package_enumeration_tests;
#[cfg(test)]
mod package_instance_tests;
#[cfg(test)]
mod source_closure_tests;
#[cfg(test)]
mod tests;

/// Absolute virtual prefix used by files extracted from a Bun standalone
/// executable. `--bunfs-root` maps the suffix below this prefix to a real
/// directory without requiring a host-level `/$bunfs` mount or symlink.
pub(super) const BUNFS_ROOT_PREFIX: &str = "/$bunfs/root/";

/// Check if a file path is inside a package that has perry.nativeLibrary in
/// its package.json. (A hard-coded list of in-tree extension packages —
/// `ethers`, `ws`, `undici` last — used to short-circuit this; those native
/// bindings were removed and the packages compile from source.)
pub(super) fn is_in_perry_native_package(path: &Path) -> bool {
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

/// Check if a file path is inside one of `compile_package_dirs` and not in a
/// nested `node_modules` tree below it (a nested dependency is its own
/// package). The directories that can contain the path are its ancestors, so
/// each ancestor is looked up in the set.
pub(super) fn is_in_compile_package_dir(
    path: &Path,
    compile_package_dirs: &BTreeSet<PathBuf>,
) -> bool {
    if compile_package_dirs.is_empty() {
        return false;
    }
    path.ancestors().any(|dir| {
        compile_package_dirs.contains(dir)
            && !path
                .strip_prefix(dir)
                .unwrap_or(path)
                .to_string_lossy()
                .contains("node_modules/")
    })
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

/// Check if a file path is inside a package listed in compile_packages.
///
/// One walk over the path's ancestors: a package directory is named by its
/// position under a `node_modules` directory (`node_modules/<name>` or
/// `node_modules/<scope>/<name>`, what [`is_compile_package_dir`] matches), so
/// each ancestor asks the set for the two names it can carry instead of
/// testing every listed package against every ancestor.
pub(super) fn is_in_compile_package(path: &Path, compile_packages: &HashSet<String>) -> bool {
    if compile_packages.is_empty() {
        return false;
    }
    path.ancestors().any(|candidate| {
        let Some(name) = candidate.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let Some(parent) = candidate.parent() else {
            return false;
        };
        let parent_name = parent.file_name().and_then(|n| n.to_str());
        if parent_name == Some("node_modules") && compile_packages.contains(name) {
            return true;
        }
        let Some(scope) = parent_name else {
            return false;
        };
        parent.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("node_modules"))
            && compile_packages.contains(&format!("{scope}/{name}"))
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

mod installed_packages;
pub(super) use installed_packages::*;

/// Every `node_modules` directory on the ancestor chain of `start`, nearest
/// first. Node's resolution algorithm consults each one until the package is
/// found — not only the nearest. pnpm layouts hit the difference constantly:
/// a package's own `node_modules` usually holds only `.bin`, while its real
/// dependencies live in the `.pnpm/<pkg>@<version>/node_modules` sibling one
/// level further up. Stopping at the first directory made every transitive
/// dependency of a compiled pnpm package unresolvable, so their call sites
/// lowered to raw extern symbols and the link failed (undefined `_toNumber` /
/// `_isUnsafe` across the fast-xml-parser graph).
pub(super) fn ancestor_node_modules_dirs(start: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = start.to_path_buf();
    loop {
        // A dir literally named `node_modules` hosts packages itself; its
        // `node_modules/node_modules` join below would never exist.
        if !current.file_name().is_some_and(|n| n == "node_modules") {
            let node_modules = current.join("node_modules");
            if node_modules.is_dir() {
                dirs.push(node_modules);
            }
        }
        if !current.pop() {
            return dirs;
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

/// TypeScript *source* counterparts for an explicit JS module extension, in
/// preference order. Node resolves an explicit `.cjs`/`.mjs` specifier to that
/// exact file, and its only TypeScript source form is `.cts`/`.mts`
/// respectively — never a bare `.ts`. Blanket-mapping every JS extension to
/// `.ts`/`.tsx`/`.mts` made `require("./x.cjs")` prefer a co-located `x.ts`
/// (e.g. a `.ts` entry requiring a same-basename `.cjs` webpack bundle), so the
/// CJS module was never compiled and its exports read back `undefined` (#6535).
/// `.js`/`.jsx` keep the broader preference Perry uses to compile a co-located
/// `.ts` source in place of a stale `.js` build artifact. Non-JS extensions
/// return an empty slice (no TS redirect).
fn ts_source_counterparts(js_ext: &str) -> &'static [&'static str] {
    match js_ext {
        "js" | "jsx" => &[".ts", ".tsx", ".mts"],
        "mjs" => &[".mts"],
        "cjs" => &[".cts"],
        _ => &[],
    }
}

/// `stem` + `ext` (`ext` includes the dot). The stem of `ts.refactor.js` is
/// `ts.refactor`; `Path::with_extension` on it would REPLACE `.refactor` and
/// resolve `./ts.refactor.js` to the sibling `./ts.ts`.
fn append_extension(stem: &Path, ext: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_os_string();
    s.push(ext);
    PathBuf::from(s)
}

/// Try to resolve a path with common extensions
/// Prefers TypeScript source files over JavaScript for native compilation
pub(super) fn resolve_with_extensions(base: &Path) -> Option<PathBuf> {
    // All extensions in order of preference (TypeScript sources first, then
    // their JS counterparts, then data). `.cts` sits with the other TS
    // extensions so a bare specifier resolves to a `.cts` source directly,
    // consistent with `.mts`/`.ts` — not only via the `.cjs` counterpart fallback.
    let all_extensions = [
        ".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".json",
    ];

    // Check if the path has an explicit JS extension - if so, try its TS source
    // counterpart(s) first (`.cjs`→`.cts`, `.mjs`→`.mts`, `.js`→`.ts`/`.tsx`).
    if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
        let counterparts = ts_source_counterparts(ext);
        if !counterparts.is_empty() {
            // Strip the JS extension and try the TS counterpart(s) first
            let stem = base.with_extension("");
            for ts_ext in counterparts {
                let ts_path = append_extension(&stem, ts_ext);
                if ts_path.exists() && ts_path.is_file() {
                    return Some(ts_path);
                }
            }
            // If no TS source found, fall back to the original JS file
            if base.exists() && base.is_file() {
                return Some(base.to_path_buf());
            }
        }
    }

    // If it already exists as-is (and not a JS file that we already handled above)
    if base.exists() && base.is_file() {
        // Even if it exists, check for a TS source counterpart first
        if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
            let counterparts = ts_source_counterparts(ext);
            if !counterparts.is_empty() {
                let stem = base.with_extension("");
                for ts_ext in counterparts {
                    let ts_path = append_extension(&stem, ts_ext);
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
            // If we landed on a JS file, prefer its co-located TS source.
            let counterparts = ts_source_counterparts(ext.trim_start_matches('.'));
            if !counterparts.is_empty() {
                for ts_ext in counterparts {
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

    // A directory is its `package.json` "main", else its index file (node's
    // LOAD_AS_DIRECTORY).
    if base.is_dir() {
        let main = fs::read_to_string(base.join("package.json"))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|pkg| pkg.get("main")?.as_str().map(PathBuf::from));
        if let Some(entry) = main.map(|main| base.join(main)) {
            if !entry.components().eq(base.components()) {
                if let Some(resolved) = resolve_with_extensions(&entry) {
                    return Some(resolved);
                }
            }
        }
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

mod package_source;
pub(super) use package_source::*;

mod exports_map;
pub(super) use exports_map::*;

mod declarations;
pub(super) use declarations::*;

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
    if !is_absolute_specifier(import_source) {
        return None;
    }
    let source_path = resolve_with_extensions(&PathBuf::from(import_source))?;
    let canonical_path = source_path.canonicalize().ok()?;
    Some(ResolvedPath {
        source_path,
        canonical_path,
    })
}

/// Return the lexical filesystem target for a Bun virtual path. Parent/root
/// components are rejected so a virtual specifier cannot escape the configured
/// extracted root.
pub(super) fn bunfs_mapped_path(import_source: &str, root: &Path) -> Option<PathBuf> {
    let suffix = import_source.strip_prefix(BUNFS_ROOT_PREFIX)?;
    let relative = Path::new(suffix);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(relative))
}

/// Resolve a Bun virtual module specifier through the configured extracted
/// root, using Perry's ordinary extension/index lookup while retaining one
/// canonical identity for real-path and virtual-path imports of the same file.
pub(super) fn resolve_bunfs_import_path(import_source: &str, root: &Path) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let mapped = bunfs_mapped_path(import_source, &canonical_root)?;
    let source_path = resolve_with_extensions(&mapped)?;
    let canonical = source_path.canonicalize().ok()?;
    canonical.starts_with(&canonical_root).then_some(canonical)
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

/// Return the absolute filesystem path that a relative import is resolved
/// from. This is intentionally available even when resolution fails so
/// diagnostics can distinguish a missing local file from a bare package with
/// no Perry bindings.
pub(super) fn attempted_relative_import_path(
    import_source: &str,
    importer_path: &Path,
) -> Option<PathBuf> {
    if !is_relative_specifier(import_source) {
        return None;
    }

    let parent = importer_path.parent().unwrap_or_else(|| Path::new(""));
    let joined = parent.join(import_source);
    let absolute = if joined.is_absolute() {
        joined
    } else {
        std::env::current_dir().ok()?.join(joined)
    };
    Some(normalize_path_lexically(&absolute))
}

pub(super) fn is_absolute_specifier(import_source: &str) -> bool {
    // Worker entries and file URLs also produce drive-letter or UNC paths.
    import_source.starts_with('/') || Path::new(import_source).is_absolute()
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
#[cfg(test)]
pub(super) fn resolve_import(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &BTreeSet<PathBuf>,
) -> Option<(PathBuf, ModuleKind)> {
    resolve_import_with_bunfs(
        import_source,
        importer_path,
        project_root,
        compile_packages,
        compile_package_dirs,
        None,
    )
}

/// Context-aware resolver entry point used by compile passes that must retain
/// `--bunfs-root` semantics after the initial module walk (export flattening,
/// init ordering, and dynamic-import metadata construction).
pub(super) fn resolve_import_with_bunfs(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &BTreeSet<PathBuf>,
    bunfs_root: Option<&Path>,
) -> Option<(PathBuf, ModuleKind)> {
    if import_source.starts_with(BUNFS_ROOT_PREFIX) {
        return bunfs_root
            .and_then(|root| resolve_bunfs_import_path(import_source, root))
            .map(|path| (path, ModuleKind::NativeCompiled));
    }
    // Check if it's a native Rust stdlib module. Refs #665: when the user has
    // explicitly opted the package into `perry.compilePackages`, they want
    // their `node_modules` copy compiled from source (cjs_wrap + native
    // codegen), not the built-in Rust FFI binding — which for some packages
    // (e.g. `rate-limiter-flexible`'s `perry-ext-ratelimit`) is incomplete.
    // The opt-in is package-scoped: bare `rate-limiter-flexible` and any
    // subpath under it both fall through to file resolution.
    let (native_check_pkg, _) = parse_package_specifier(import_source);
    if perry_hir::is_native_module_specifier(import_source)
        && !compile_packages.contains(&native_check_pkg)
    {
        return None; // Native modules are handled by stdlib, not file imports
    }

    // Node subpath imports (`#…`, #5039): resolve through the importing
    // package's own `package.json` `"imports"` map with full
    // PACKAGE_IMPORTS_RESOLVE semantics (see subpath_imports.rs), then
    // classify exactly like a relative import to the mapped file. This runs
    // BEFORE the tsconfig-paths fallback so spec-defined resolution wins over
    // tsconfig aliasing; when no `imports` field governs the importer the
    // specifier falls through, so a tsconfig `paths` alias covering `#…`
    // keeps working.
    let subpath_import_target = if import_source.starts_with('#') {
        use subpath_imports::SubpathImportOutcome;
        match subpath_imports::resolve_subpath_import(
            import_source,
            importer_path,
            subpath_imports::default_conditions(),
        ) {
            Ok(SubpathImportOutcome::File(canonical)) => Some(canonical),
            // Bare-package target: re-enter resolution with the mapped
            // specifier, which resolves through node_modules per spec (or the
            // stdlib for `node:` builtins).
            Ok(SubpathImportOutcome::External(spec)) => {
                return resolve_import_with_bunfs(
                    &spec,
                    importer_path,
                    project_root,
                    compile_packages,
                    compile_package_dirs,
                    bunfs_root,
                );
            }
            // Not covered by an `imports` map — fall through (the tsconfig
            // `paths` fallback below may still alias it).
            Ok(SubpathImportOutcome::NotDefined) => None,
            // Spec-defined hard errors (`#` / `#/…`, escaping targets, …)
            // must be surfaced, not silently degraded to tsconfig aliasing.
            Err(err) => {
                eprintln!(
                    "warning: cannot resolve '{}' (imported from {}): {}",
                    import_source,
                    importer_path.display(),
                    err
                );
                return None;
            }
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
                || is_in_compile_package_dir(&canonical, compile_package_dirs);
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
    if is_absolute_specifier(import_source) {
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

    // Bare package imports are importer-relative. Start at the importing
    // module and walk its ancestors exactly as Node/Bun do; the project root
    // remains an additive fallback for lexical/canonical path edge cases.
    // A compilePackages opt-in controls how the resolved copy is compiled, not
    // which installed copy wins resolution.
    let search_paths = [importer_path.parent(), Some(project_root)];

    for start in search_paths.iter().flatten() {
        for node_modules in ancestor_node_modules_dirs(start) {
            let package_dir = node_modules.join(&package_name);
            if !package_dir.is_dir() {
                continue;
            }
            let Some(entry) = resolve_package_entry(&package_dir, subpath.as_deref()) else {
                continue;
            };
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
                // Prefer TypeScript source over compiled JS
                if let Some(src_entry) =
                    resolve_package_source_entry(&package_dir, subpath.as_deref())
                {
                    return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                }
                // Fall back to normal resolution but still mark as NativeCompiled
                if let Some(fallback_entry) =
                    resolve_package_entry(&package_dir, subpath.as_deref())
                {
                    return Some((
                        fallback_entry.canonicalize().ok()?,
                        ModuleKind::NativeCompiled,
                    ));
                }
                // The entry resolved above is the same package instance and is
                // kept as the final fallback for unusual package metadata.
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
                .iter()
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
/// The symbol prefix of the module at `resolved_path`. `overrides` holds the
/// prefixes [`super::module_prefixes::disambiguate`] gave modules whose
/// sanitized names collide; every other module keeps its sanitized
/// project-relative path.
pub(super) fn compute_module_prefix(
    resolved_path: &str,
    project_root: &Path,
    overrides: &HashMap<String, String>,
) -> String {
    if let Some(prefix) = overrides.get(resolved_path) {
        return prefix.clone();
    }
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
    perry_codegen::symbol_names::module_prefix(&source_module_name)
}

/// Cached wrapper around resolve_import to avoid redundant I/O
pub(super) fn cached_resolve_import(
    import_source: &str,
    importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<(PathBuf, ModuleKind)> {
    if let Some(generated) = ctx.generated_asset_modules.get(import_source) {
        return Some((generated.source_path.clone(), ModuleKind::NativeCompiled));
    }
    let importer_dir = importer_path
        .parent()
        .unwrap_or(importer_path)
        .to_path_buf();
    let cache_key = (import_source.to_string(), importer_dir);
    if let Some(cached) = ctx.resolve_cache.get(&cache_key) {
        return cached.clone();
    }
    let result = resolve_import_with_context(import_source, importer_path, ctx);
    ctx.resolve_cache.insert(cache_key, result.clone());
    result
}
