//! Compile-package Node native-addon detection.
//!
//! Extracted from `collect_modules.rs` (file-size cap). A package listed in
//! `perry.compilePackages` must be pure JS/TS — Perry cannot load Node
//! `.node` / N-API addons inside a native binary. These helpers locate the
//! package root for a resolved file and probe it for native-addon markers
//! (`binding.gyp`, `prebuilds/`, `gypfile`, `node-gyp-build`/`bindings`
//! loader deps, or a stray `*.node`), so `refuse_compile_package_native_addon`
//! can fail the compile with an actionable message instead of silently
//! emitting a broken binary.

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

// Parent (`collect_modules`) private imports are visible to this child module.
use super::has_perry_native_library;
use super::CompilationContext;

fn nearest_package_root(path: &std::path::Path) -> Option<PathBuf> {
    let mut dir = path.parent();
    while let Some(candidate) = dir {
        if candidate.join("package.json").exists() {
            return Some(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

fn package_root_for_compile_package(
    ctx: &CompilationContext,
    path: &std::path::Path,
) -> Option<PathBuf> {
    ctx.compile_package_dirs
        .values()
        .filter(|dir| path.starts_with(dir))
        .max_by_key(|dir| dir.components().count())
        .cloned()
        .or_else(|| nearest_package_root(path))
}

fn package_name_from_package_json(package_root: &std::path::Path) -> Option<String> {
    let package_json = fs::read_to_string(package_root.join("package.json")).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&package_json).ok()?;
    parsed
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn find_node_addon_file(dir: &std::path::Path, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name == "node_modules" || file_name == ".git" {
            continue;
        }
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("node") {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_node_addon_file(&path, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn node_addon_marker(package_root: &std::path::Path) -> Option<(&'static str, String)> {
    let binding_gyp = package_root.join("binding.gyp");
    if binding_gyp.exists() {
        return Some(("binding.gyp", binding_gyp.display().to_string()));
    }
    let prebuilds = package_root.join("prebuilds");
    if prebuilds.is_dir() {
        return Some(("prebuilds/", prebuilds.display().to_string()));
    }
    let package_json_path = package_root.join("package.json");
    if let Ok(package_json) = fs::read_to_string(&package_json_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&package_json) {
            if parsed
                .get("gypfile")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return Some((
                    "package.json gypfile",
                    package_json_path.display().to_string(),
                ));
            }
            if package_json_dependency_uses_native_addon_loader(&parsed, "node-gyp-build")
                || package_json_dependency_uses_native_addon_loader(&parsed, "bindings")
            {
                return Some((
                    "native addon loader dependency",
                    package_json_path.display().to_string(),
                ));
            }
        }
    }
    if let Some(node_file) = find_node_addon_file(package_root, 5) {
        return Some(("*.node", node_file.display().to_string()));
    }
    None
}

fn package_json_dependency_uses_native_addon_loader(
    package_json: &serde_json::Value,
    loader_name: &str,
) -> bool {
    ["dependencies", "optionalDependencies"]
        .iter()
        .any(|section| {
            package_json
                .get(section)
                .and_then(|deps| deps.as_object())
                .is_some_and(|deps| deps.contains_key(loader_name))
        })
}

pub(super) fn refuse_compile_package_native_addon(
    ctx: &mut CompilationContext,
    canonical: &std::path::Path,
) -> Result<()> {
    let Some(package_root) = package_root_for_compile_package(ctx, canonical) else {
        return Ok(());
    };
    if !ctx
        .checked_compile_package_native_addon_roots
        .insert(package_root.clone())
    {
        return Ok(());
    }
    if has_perry_native_library(&package_root) {
        return Ok(());
    }
    let Some((marker, marker_path)) = node_addon_marker(&package_root) else {
        return Ok(());
    };
    let package_name = package_name_from_package_json(&package_root)
        .unwrap_or_else(|| package_root.display().to_string());
    anyhow::bail!(
        "package `{}` is in `perry.compilePackages` but uses a Node native addon ({}) at {}.\n\
         Perry cannot load Node `.node` / N-API addons inside a native Perry binary. \
         Remove `{}` from `perry.compilePackages`, choose a pure JS/TS package, \
         or replace the native boundary with a Perry native binding \
         (`perry.nativeLibrary` / perry-ffi).",
        package_name,
        marker,
        marker_path,
        package_name
    );
}
