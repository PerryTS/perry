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
use std::path::{Path, PathBuf};

use super::{
    CompilationContext, NativeFunctionDecl, NativeLibraryManifest, TargetNativeConfig,
};

/// Find the Perry workspace root by searching upward from the executable location.
pub fn find_perry_workspace_root() -> Option<PathBuf> {
    // First try: relative to the perry executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Binary in target/release/ → workspace is ../../
            for ancestor in [dir, &dir.join(".."), &dir.join("../.."), &dir.join("../../..")] {
                let candidate = std::fs::canonicalize(ancestor).ok()?;
                if candidate.join("crates/perry-runtime").is_dir()
                    && candidate.join("crates/perry-ui-geisterhand").is_dir()
                {
                    return Some(candidate);
                }
            }
        }
    }
    // Second try: current working directory or its ancestors
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if dir.join("crates/perry-runtime").is_dir()
                && dir.join("crates/perry-ui-geisterhand").is_dir()
            {
                return Some(dir.to_path_buf());
            }
            dir = dir.parent()?;
        }
    }
    None
}

/// Check if a package directory has a perry.nativeLibrary field in its package.json
pub(super) fn has_perry_native_library(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg.get("perry")
                .and_then(|p| p.get("nativeLibrary"))
                .is_some();
        }
    }
    false
}

/// Check if a package directory has `perry.nativeModule: true` in its package.json.
///
/// Packages that set this flag contain Perry-compatible TypeScript source code
/// and should be compiled natively (NativeCompiled) rather than interpreted via V8.
/// This is the mechanism used by `perry-react`, `perry-react-dom`, and similar
/// first-party TypeScript packages that rely on `perry/ui` or other native modules.
pub(super) fn has_perry_native_module(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg.get("perry")
                .and_then(|p| p.get("nativeModule"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
    }
    false
}

/// Parse a native library manifest from a package's package.json
pub(super) fn parse_native_library_manifest(
    package_dir: &Path,
    module_name: &str,
    target: Option<&str>,
) -> Option<NativeLibraryManifest> {
    let package_json = package_dir.join("package.json");
    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    let native_lib = pkg.get("perry")?.get("nativeLibrary")?;

    // Parse functions
    let functions: Vec<NativeFunctionDecl> = native_lib.get("functions")?
        .as_array()?
        .iter()
        .filter_map(|f| {
            Some(NativeFunctionDecl {
                name: f.get("name")?.as_str()?.to_string(),
                params: f.get("params")?
                    .as_array()?
                    .iter()
                    .filter_map(|p| p.as_str().map(|s| s.to_string()))
                    .collect(),
                returns: f.get("returns")?.as_str()?.to_string(),
            })
        })
        .collect();

    // Parse target config
    let target_key = match target {
        Some("ios-simulator") | Some("ios") => "ios",
        Some("visionos-simulator") | Some("visionos") => "visionos",
        Some("android") => "android",
        Some("tvos-simulator") | Some("tvos") => "tvos",
        Some("watchos-simulator") | Some("watchos") => "watchos",
        Some("harmonyos-simulator") | Some("harmonyos") => "harmonyos",
        Some("linux") => "linux",
        Some("windows") => "windows",
        Some("web") => "web",
        None if cfg!(target_os = "linux") => "linux",
        None if cfg!(target_os = "windows") => "windows",
        _ => "macos",
    };

    let target_config = native_lib.get("targets")
        .and_then(|t| t.get(target_key))
        .map(|tc| {
            TargetNativeConfig {
                crate_path: package_dir.join(
                    tc.get("crate").and_then(|c| c.as_str()).unwrap_or("")
                ),
                lib_name: tc.get("lib").and_then(|l| l.as_str())
                    .unwrap_or("").to_string(),
                frameworks: tc.get("frameworks")
                    .and_then(|f| f.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                libs: tc.get("libs")
                    .and_then(|l| l.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                pkg_config: tc.get("pkgConfig")
                    .and_then(|p| p.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                swift_sources: tc.get("swift_sources")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter()
                        .filter_map(|v| v.as_str().map(|p| package_dir.join(p)))
                        .collect())
                    .unwrap_or_default(),
                metal_sources: tc.get("metal_sources")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter()
                        .filter_map(|v| v.as_str().map(|p| package_dir.join(p)))
                        .collect())
                    .unwrap_or_default(),
            }
        });

    Some(NativeLibraryManifest {
        module: module_name.to_string(),
        package_dir: package_dir.to_path_buf(),
        functions,
        target_config,
    })
}

/// Packages that Perry provides built-in native extensions for.
/// These must never be loaded into V8 — Perry's codegen intercepts all imports
/// from these packages and replaces them with native calls.
const PERRY_NATIVE_EXTENSION_PACKAGES: &[&str] = &[
    "ioredis", "ethers", "mysql2", "ws", "dotenv",
];

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
        if dir.file_name().map(|n| n == "node_modules").unwrap_or(false) {
            break;
        }
        current = dir.parent();
    }
    false
}

/// Extract the package directory from a resolved path for a given package name.
/// E.g., for path "/project/node_modules/@noble/curves/node_modules/@noble/hashes/src/sha256.ts"
/// and package_name "@noble/hashes", returns "/project/node_modules/@noble/curves/node_modules/@noble/hashes"
pub(super) fn extract_compile_package_dir(resolved_path: &Path, package_name: &str) -> Option<PathBuf> {
    let path_str = resolved_path.to_string_lossy();
    let needle = format!("node_modules/{}", package_name);
    // Use rfind to handle deeply nested node_modules
    if let Some(idx) = path_str.rfind(&needle) {
        Some(PathBuf::from(&path_str[..idx + needle.len()]))
    } else {
        None
    }
}

/// Check if a file path is inside a package listed in compile_packages
pub(super) fn is_in_compile_package(path: &Path, compile_packages: &HashSet<String>) -> bool {
    let path_str = path.to_string_lossy();
    for pkg_name in compile_packages {
        let pattern = format!("node_modules/{}/", pkg_name);
        if path_str.contains(&pattern) {
            return true;
        }
    }
    false
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

    // Try with extensions in order of preference (TS before JS)
    for ext in all_extensions {
        let with_ext = base.with_extension(ext.trim_start_matches('.'));
        if with_ext.exists() && with_ext.is_file() {
            return Some(with_ext);
        }

        // Also try adding extension to full path (for paths like ./foo.js)
        let path_str = base.to_string_lossy();
        let with_ext = PathBuf::from(format!("{}{}", path_str, ext));
        if with_ext.exists() && with_ext.is_file() {
            // If we found a JS file, check for TS equivalent first
            if matches!(ext, ".js" | ".mjs" | ".cjs") {
                let stem_str = path_str.to_string();
                for ts_ext in ts_extensions {
                    let ts_path = PathBuf::from(format!("{}{}", stem_str, ts_ext));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
            return Some(with_ext);
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
        if let Some(entry) = resolve_exports(exports, &export_key) {
            let entry_path = package_dir.join(&entry);
            if entry_path.exists() {
                return Some(entry_path);
            }
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
pub(super) fn resolve_package_source_entry(package_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
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
                if let Some(rest) = stripped.ok() {
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
pub(super) fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            // Try the specific subpath first
            if let Some(entry) = map.get(subpath) {
                return resolve_exports(entry, subpath);
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
                            if let Some(template) = resolve_exports(value, subpath) {
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
            for condition in ["perry", "import", "module", "default", "require", "node"] {
                if let Some(entry) = map.get(condition) {
                    return resolve_exports(entry, subpath);
                }
            }

            None
        }
        _ => None,
    }
}

/// Determine if a file is a JavaScript file (not TypeScript)
pub(super) fn is_js_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "js" | "mjs" | "cjs")
    } else {
        false
    }
}

/// Determine if a file is a TypeScript declaration file (.d.ts)
pub(super) fn is_declaration_file(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".d.ts")
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

/// Resolve an import specifier to a file path
pub(super) fn resolve_import(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &HashMap<String, PathBuf>,
) -> Option<(PathBuf, ModuleKind)> {
    // Check if it's a native Rust stdlib module
    if perry_hir::is_native_module(import_source) {
        return None; // Native modules are handled by stdlib, not file imports
    }

    // Handle relative imports (./ or ../)
    if import_source.starts_with("./") || import_source.starts_with("../") {
        let parent = importer_path.parent()?;
        let resolved = parent.join(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let kind = if is_js_file(&path) && !is_in_compile_package(&path, compile_packages) {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((path.canonicalize().ok()?, kind));
        }
        return None;
    }

    // Handle absolute paths
    if import_source.starts_with('/') {
        let resolved = PathBuf::from(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let kind = if is_js_file(&path) {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((path.canonicalize().ok()?, kind));
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
                        if let Some(src_entry) = resolve_package_source_entry(effective_dir, subpath.as_deref()) {
                            return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                        }
                        // Fall back to normal resolution but still mark as NativeCompiled
                        if let Some(fallback_entry) = resolve_package_entry(effective_dir, subpath.as_deref()) {
                            return Some((fallback_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                        }
                        // If effective_dir failed (shouldn't happen), try the local dir
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // For other node_modules packages, treat as Interpreted
                    // Even .ts files in node_modules are library source code,
                    // not user code to be compiled. V8 will handle them at runtime.
                    return Some((entry.canonicalize().ok()?, ModuleKind::Interpreted));
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
                    if let Some(src_entry) = resolve_package_source_entry(&file_dep_dir, subpath.as_deref()) {
                        return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    if let Some(fallback_entry) = resolve_package_entry(&file_dep_dir, subpath.as_deref()) {
                        return Some((fallback_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                }
                return Some((entry.canonicalize().ok()?, ModuleKind::Interpreted));
            }
        }
    }

    None
}

/// Discover extension entry points from a directory of plugins.
/// Each subdirectory is checked for a package.json with an `openclaw.extensions` array.
/// Returns Vec<(entry_path, plugin_id)> — e.g., ("extensions/telegram/index.ts", "telegram").
pub(super) fn discover_extension_entries(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = Vec::new();

    if !dir.is_dir() {
        return Err(anyhow!("--bundle-extensions path is not a directory: {}", dir.display()));
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let subdir = entry.path();
        if !subdir.is_dir() {
            continue;
        }

        let plugin_id = subdir.file_name()
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

            let extensions = pkg.get("openclaw")
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
        .unwrap_or_else(|| source_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("module")
            .to_string());
    let mut prefix = source_module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
    // LLVM IR identifiers cannot start with a digit. Prefix with `_`
    // if the first character would be one (e.g. `05_fibonacci.ts`).
    if prefix.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
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
    let importer_dir = importer_path.parent().unwrap_or(importer_path).to_path_buf();
    let cache_key = (import_source.to_string(), importer_dir);
    if let Some(cached) = ctx.resolve_cache.get(&cache_key) {
        return cached.clone();
    }
    let result = resolve_import(import_source, importer_path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs);
    ctx.resolve_cache.insert(cache_key, result.clone());
    result
}
